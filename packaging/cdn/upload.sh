#!/usr/bin/env bash
# Puts a release's files on the download CDN: Cloudflare R2 (bucket $BUCKET), served at https://$HOST/.
#   packaging/cdn/upload.sh TAG DIR [--latest]
# DIR holds the release's files (SHA256SUMS among them). Each goes to TAG/NAME, then TAG/release.json (the files, their
# sizes and checksums); --latest also points latest.json at it (the website's download links). Already uploaded
# files are put again: the same names, the same contents.
# Needs CLOUDFLARE_API_TOKEN (Workers R2 Storage: Edit) and CLOUDFLARE_ACCOUNT_ID; npx (Node) and jq.
set -euo pipefail

BUCKET=${BUCKET:-openkumite-releases}
HOST=${HOST:-download.openkumiteengine.dev}
WRANGLER=${WRANGLER:-4.163.0}

tag=${1:?TAG}; dir=${2:?DIR}; latest=${3:-}
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "not a version tag: $tag" >&2; exit 1; }
[ -f "$dir/SHA256SUMS" ] || { echo "no $dir/SHA256SUMS" >&2; exit 1; }
(cd "$dir" && sha256sum -c --quiet SHA256SUMS 2>/dev/null || shasum -a 256 -c --quiet SHA256SUMS)

put() { # KEY FILE CACHE-CONTROL [CONTENT-TYPE]
  local args=(r2 object put "$BUCKET/$1" --file "$2" --cache-control "$3" --remote)
  [ -n "${4:-}" ] && args+=(--content-type "$4")
  case "$1" in *.json) ;; *) args+=(--content-disposition "attachment; filename=\"$(basename "$1")\"") ;; esac
  for try in 1 2 3; do
    npx --yes "wrangler@$WRANGLER" "${args[@]}" >/dev/null && return 0
    echo "retrying $1 ($try)" >&2; sleep $((try * 5))
  done
  return 1
}

files=()
while read -r sum name; do
  name=${name#\*}
  files+=("$(jq -n --arg n "$name" --arg s "$sum" --argjson b "$(wc -c < "$dir/$name" | tr -d ' ')" \
    --arg u "https://$HOST/$tag/$name" '{name: $n, size: $b, sha256: $s, url: $u}')")
done < "$dir/SHA256SUMS"

for f in "$dir"/*; do
  n=$(basename "$f"); type=""
  case "$n" in
    *.apk) [[ "$n" == *android* ]] && type=application/vnd.android.package-archive ;;
    SHA256SUMS) type="text/plain; charset=utf-8" ;;
  esac
  echo "$tag/$n"
  put "$tag/$n" "$f" "public, max-age=31536000, immutable" "$type"
done

manifest=$(mktemp)
printf '%s\n' "${files[@]}" | jq -s --arg t "$tag" --arg d "${PUBLISHED:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}" --arg h "https://$HOST" \
  '{tag: $t, prerelease: ($t | test("-")), published: $d, base: "\($h)/\($t)/",
    checksums: "\($h)/\($t)/SHA256SUMS", files: .}' > "$manifest"
put "$tag/release.json" "$manifest" "public, max-age=300" "application/json"
if [ "$latest" = --latest ]; then
  put latest.json "$manifest" "public, max-age=300" "application/json"
  echo "latest.json -> $tag"
fi
rm -f "$manifest"
echo "https://$HOST/$tag/"
