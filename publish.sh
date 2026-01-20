#!/bin/bash
set -e

# Usage: ./publish.sh <version>
# Example: ./publish.sh 0.1.9

if [ -z "$1" ]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 0.1.9"
    exit 1
fi

VERSION="$1"
TAG="v$VERSION"

echo "Updating all versions to $VERSION..."

# Update workspace version in root Cargo.toml
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml

# Update ndl-core dependency version in ndl/Cargo.toml and ndld/Cargo.toml
sed -i "s/ndl-core = { path = \"..\/ndl-core\", version = \".*\" }/ndl-core = { path = \"..\/ndl-core\", version = \"$VERSION\" }/" ndl/Cargo.toml
sed -i "s/ndl-core = { path = \"..\/ndl-core\", version = \".*\" }/ndl-core = { path = \"..\/ndl-core\", version = \"$VERSION\" }/" ndld/Cargo.toml

echo "Verifying build..."
cargo check --workspace

echo "Committing version bump..."
git add Cargo.toml ndl/Cargo.toml ndld/Cargo.toml Cargo.lock
git commit -m "$VERSION"

echo "Creating tag $TAG..."
git tag "$TAG"

echo "Publishing ndl-core..."
cargo publish --package ndl-core

echo "Waiting for crates.io to index ndl-core..."
sleep 15

echo "Publishing ndl..."
cargo publish --package ndl

echo "Pushing commits and tags..."
git push
git push --tags

echo "Done! Published ndl-core and ndl version $VERSION"
echo ""
echo "To also publish ndld, run:"
echo "  cargo publish --package ndld"
