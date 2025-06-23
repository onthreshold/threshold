#!/bin/bash
set -e

# This script finds all json files in the keys directory and removes the "dkg_keys" entry from them.

# Check if jq is installed
if ! command -v jq &> /dev/null
then
    echo "jq could not be found. Please install jq to run this script." >&2
    exit 1
fi

# Find all json files in the keys directory and its subdirectories
# and remove the "dkg_keys" entry from them using a temporary file.
find keys -type f -name "*.json" -print0 | while IFS= read -r -d $'\0' file; do
    echo "Processing $file"
    # Create a temp file in the same directory to avoid issues with `mv` across filesystems
    tmpfile="${file}.tmp"
    jq 'del(.dkg_keys)' "$file" > "$tmpfile"
    mv "$tmpfile" "$file"
done

# Find all json files in the keys directory and its subdirectories
# and remove the "dkg_keys" entry from them using a temporary file.
find test_artifacts -type f -name "*.json" -print0 | while IFS= read -r -d $'\0' file; do
    echo "Processing $file"
    # Create a temp file in the same directory to avoid issues with `mv` across filesystems
    tmpfile="${file}.tmp"
    jq 'del(.dkg_keys)' "$file" > "$tmpfile"
    mv "$tmpfile" "$file"
done

echo "Done. dkg_keys removed from all json files in the keys/ directory."
