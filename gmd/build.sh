#!/bin/sh
# Builds gmd for every target into gmd/dist/, and writes gmd/release.txt: the release tag
# then one "<asset> <sha256>" line per target, what build.rs downloads and checks.
#
#   gmd/build.sh gmd-v0.1.0
#
# Builds are reproducible (-trimpath, no build id, no cgo): the same Go version gives the
# same files, so anyone can check the published hashes.
set -eu
tag=${1:?usage: gmd/build.sh <tag, e.g. gmd-v0.1.0>}
cd "$(dirname "$0")"
rm -rf dist
mkdir dist
for target in linux/amd64 linux/arm64 darwin/amd64 darwin/arm64 windows/amd64 windows/arm64; do
    os=${target%/*}
    arch=${target#*/}
    ext=
    [ "$os" = windows ] && ext=.exe
    CGO_ENABLED=0 GOOS=$os GOARCH=$arch go build -trimpath -ldflags="-s -w -buildid=" \
        -o "dist/gmd-$os-$arch$ext" .
done
{
    echo "$tag"
    (cd dist && sha256sum gmd-*) | awk '{print $2, $1}'
} > release.txt
cat release.txt
