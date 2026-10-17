"""GitHub's REST API through the gh CLI (`gh api`). Only REST: GraphQL is
not available everywhere the swarm runs."""

import io
import json
import re
import subprocess
import zipfile


class GitHubError(RuntimeError):
    pass


class GitHub:
    def __init__(self, repo):
        self.repo = repo  # "owner/name"

    def api(self, method, path, body=None, raw=False):
        cmd = ["gh", "api", "-X", method, f"repos/{self.repo}/{path}"]
        if body is not None:
            cmd += ["--input", "-"]
        r = subprocess.run(cmd, input=json.dumps(body).encode() if body is not None else None,
                           capture_output=True, timeout=300)
        if r.returncode != 0:
            msg = (r.stdout or b"").decode(errors="replace")[:300] + (r.stderr or b"").decode(errors="replace")[:300]
            raise GitHubError(f"{method} {path}: {msg.strip()}")
        if raw:
            return r.stdout
        text = r.stdout.decode(errors="replace").strip()
        return json.loads(text) if text else None

    # Pull requests

    def create_pr(self, base, head, title, body):
        return self.api("POST", "pulls", {"base": base, "head": head, "title": title, "body": body})["number"]

    def pr(self, number):
        return self.api("GET", f"pulls/{number}")

    def update_pr(self, number, **fields):
        return self.api("PATCH", f"pulls/{number}", fields)

    def comment(self, number, body):
        self.api("POST", f"issues/{number}/comments", {"body": body})

    def add_labels(self, number, labels):
        self.api("POST", f"issues/{number}/labels", {"labels": labels})

    # CI

    def check_runs(self, sha):
        return self.api("GET", f"commits/{sha}/check-runs?per_page=100")["check_runs"]

    @staticmethod
    def run_id(check_run):
        m = re.search(r"/actions/runs/(\d+)", check_run.get("details_url") or "")
        return int(m.group(1)) if m else None

    def artifact(self, run_id, name):
        """The named artifact of a workflow run, unzipped to {filename: bytes},
        or None if the run has no such artifact."""
        arts = self.api("GET", f"actions/runs/{run_id}/artifacts?name={name}")["artifacts"]
        arts = [a for a in arts if not a["expired"]]
        if not arts:
            return None
        data = self.api("GET", f"actions/artifacts/{arts[0]['id']}/zip", raw=True)
        with zipfile.ZipFile(io.BytesIO(data)) as z:
            return {n: z.read(n) for n in z.namelist()}

    def rerun_failed(self, run_id):
        self.api("POST", f"actions/runs/{run_id}/rerun-failed-jobs")

    # Stacks (the API `gh stack` uses)

    def stack_for_pr(self, number):
        stacks = self.api("GET", f"stacks?pull_request={number}") or []
        return stacks[0] if stacks else None

    def create_stack(self, prs):
        return self.api("POST", "stacks", {"pull_requests": prs})

    def add_to_stack(self, stack, prs):
        return self.api("POST", f"stacks/{stack}/add", {"pull_requests": prs})

    def unstack(self, stack):
        return self.api("POST", f"stacks/{stack}/unstack")
