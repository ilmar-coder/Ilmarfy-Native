#!/bin/bash
# Publica uma versão do app Android no servidor; os celulares recebem o aviso de atualização ao abrir o app.
#
#   ./release.sh "Notas da versão"                 publica a versão atual de version.properties
#   ./release.sh --bump patch "Notas da versão"    0.2.0 -> 0.2.1 (ou --bump minor: 0.2.0 -> 0.3.0)
#
# Requisitos: chave fixa em local.properties (ilmarfy.keystore) e acesso SSH ao servidor por chave.
set -euo pipefail
cd "$(dirname "$0")"

SERVER="${DEPLOY_SERVER:-ilmar@192.168.0.69}"
REMOTE_DIR="/www/wwwroot/baixar-go/data/apps/ilmarfy-android"
export JAVA_HOME="${JAVA_HOME:-/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home}"

prop() { grep "^$1=" version.properties | cut -d= -f2; }

if [ "${1:-}" = "--bump" ]; then
    part="${2:?use --bump patch|minor}"
    shift 2
    IFS=. read -r major minor patch <<< "$(prop versionName)"
    case "$part" in
        patch) patch=$((patch + 1)) ;;
        minor) minor=$((minor + 1)); patch=0 ;;
        *) echo "use --bump patch|minor"; exit 1 ;;
    esac
    printf 'versionCode=%s\nversionName=%s\n' "$(($(prop versionCode) + 1))" "$major.$minor.$patch" > version.properties
fi
NOTES="${1:-}"
CODE=$(prop versionCode)
NAME=$(prop versionName)

grep -q '^ilmarfy.keystore=' local.properties 2>/dev/null || { echo "Falta ilmarfy.keystore em local.properties (a chave fixa)."; exit 1; }

PUBLISHED=$({ ssh -o BatchMode=yes "$SERVER" "cat $REMOTE_DIR/latest.json 2>/dev/null" || true; } | python3 -c 'import json,sys; d=sys.stdin.read(); print(json.loads(d)["version_code"] if d.strip() else 0)')
if [ "$CODE" -le "$PUBLISHED" ]; then
    echo "O servidor já tem a versão $PUBLISHED; aumente a versão (./release.sh --bump patch \"notas\")."
    exit 1
fi

echo "==> Compilando Ilmarfy $NAME ($CODE)..."
./gradlew -q --console=plain assembleRelease
APK=build.nosync/app/outputs/apk/release/app-release.apk
SIGNER=$(ls -d "$HOME/Library/Android/sdk/build-tools/"* | sort -V | tail -1)/apksigner
"$SIGNER" verify "$APK"

FILE="ilmarfy-$NAME.apk"
SIZE=$(stat -f%z "$APK")
SHA=$(shasum -a 256 "$APK" | cut -d' ' -f1)
python3 - "$CODE" "$NAME" "$NOTES" "$FILE" "$SIZE" "$SHA" > /tmp/ilmarfy-latest.json <<'EOF'
import json, sys, time
code, name, notes, file, size, sha = sys.argv[1:]
print(json.dumps({"version_code": int(code), "version_name": name, "notes": notes, "file": file,
                  "size": int(size), "sha256": sha, "published_at": int(time.time())}, ensure_ascii=False, indent=2))
EOF

echo "==> Publicando no servidor..."
ssh -o BatchMode=yes "$SERVER" "mkdir -p $REMOTE_DIR"
scp -q "$APK" "$SERVER:$REMOTE_DIR/$FILE"
scp -q /tmp/ilmarfy-latest.json "$SERVER:$REMOTE_DIR/latest.json.tmp"
# Troca atômica do latest.json e mantém só as 3 versões mais novas
ssh -o BatchMode=yes "$SERVER" "cd $REMOTE_DIR && mv latest.json.tmp latest.json && ls -t ilmarfy-*.apk | tail -n +4 | xargs -r rm -f"
rm -f /tmp/ilmarfy-latest.json
cp "$APK" "$HOME/Downloads/$FILE"

echo "✅ Ilmarfy $NAME ($CODE) publicado. Os celulares vão receber o aviso ao abrir o app."
echo "   Cópia: ~/Downloads/$FILE"
