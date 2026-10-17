#!/bin/sh
# Fetches react-native@VERSION from npm and extracts the parts barre's RN
# shim works on (ReactCommon/yoga) into DEST, laid out as in the package
# (DEST/ReactCommon/yoga), so rn/patches/*.patch applies with `patch -p1 -d DEST`.
#
#   rn/fetch-react-native.sh 0.88.0 build/rn/react-native-0.88.0
set -eu
version=$1
dest=$2
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
(cd "$tmp" && npm pack --silent "react-native@$version" >/dev/null)
mkdir -p "$tmp/x"
tar -xzf "$tmp"/react-native-*.tgz -C "$tmp/x" package/ReactCommon/yoga package/package.json
rm -rf "$dest"
mkdir -p "$dest/ReactCommon"
mv "$tmp/x/package/ReactCommon/yoga" "$dest/ReactCommon/yoga"
mv "$tmp/x/package/package.json" "$dest/package.json"
