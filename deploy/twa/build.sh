#!/bin/bash
# =============================================================================
# Dinotty TWA (Trusted Web Activity) APK builder
# =============================================================================
# Builds a signed debug APK without Android Studio or Gradle.
# Pipeline: aapt2 compile/link -> javac -> d8 -> jar uf -> zipalign -> apksigner
#
# Required env:
#   ANDROID_HOME or ANDROID_SDK_ROOT  (must contain build-tools/34.0.0 and
#                                      platforms/android-34/android.jar)
#   JAVA_HOME                          (JDK 17 recommended)
#
# Required SDK packages (install via sdkmanager BEFORE running):
#   sdkmanager "platforms;android-35" "build-tools;35.0.0"
# =============================================================================

set -e
set -u

# ---------- Resolve project root (one level up from deploy/twa) ----------
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TWADIR="$SCRIPT_DIR"
OUTDIR="$TWADIR/build"
KEYSTORE="$TWADIR/debug.keystore"

# ---------- Environment checks ----------
if [[ -z "${ANDROID_HOME:-}" && -z "${ANDROID_SDK_ROOT:-}" ]]; then
    echo "ERROR: neither ANDROID_HOME nor ANDROID_SDK_ROOT is set." >&2
    echo "       Export one of them, e.g.: export ANDROID_HOME=/opt/android-sdk" >&2
    exit 1
fi
SDK="${ANDROID_HOME:-$ANDROID_SDK_ROOT}"

if [[ -z "${JAVA_HOME:-}" ]]; then
    echo "ERROR: JAVA_HOME is not set." >&2
    echo "       Export JDK path, e.g.: export JAVA_HOME=/usr/lib/jvm/java-17-openjdk-amd64" >&2
    exit 1
fi

BUILD_TOOLS_VER="35.0.0"
PLATFORM_VER="35"
BT="$SDK/build-tools/$BUILD_TOOLS_VER"
PLATFORM_JAR="$SDK/platforms/android-$PLATFORM_VER/android.jar"

AAPT2="$BT/aapt2"
D8="$BT/d8"
ZIPALIGN="$BT/zipalign"
APKSIGNER="$BT/apksigner"
JAVAC="$JAVA_HOME/bin/javac"
JAR="$JAVA_HOME/bin/jar"
KEYTOOL="$JAVA_HOME/bin/keytool"

# ---------- Tool availability checks ----------
require_file() {
    local p="$1"
    if [[ ! -x "$p" && ! -f "$p" ]]; then
        echo "ERROR: required file not found: $p" >&2
        echo "       Install via: sdkmanager \"$2\"" >&2
        exit 1
    fi
}

require_file "$AAPT2"      "build-tools;$BUILD_TOOLS_VER"
require_file "$D8"         "build-tools;$BUILD_TOOLS_VER"
require_file "$ZIPALIGN"   "build-tools;$BUILD_TOOLS_VER"
require_file "$APKSIGNER"  "build-tools;$BUILD_TOOLS_VER"
require_file "$JAVAC"      "platforms;android-$PLATFORM_VER (provides javac)"
require_file "$JAR"        "platforms;android-$PLATFORM_VER (provides jar)"
require_file "$KEYTOOL"    "platforms;android-$PLATFORM_VER (provides keytool)"
require_file "$PLATFORM_JAR" "platforms;android-$PLATFORM_VER"

echo "=== Dinotty TWA build ==="
echo "SDK         : $SDK"
echo "Build-tools : $BUILD_TOOLS_VER"
echo "Platform    : android-$PLATFORM_VER"
echo "JAVA_HOME   : $JAVA_HOME"
echo "TWADIR      : $TWADIR"
echo "OUTDIR      : $OUTDIR"
echo

# ---------- Clean and prepare output ----------
rm -rf "$OUTDIR"
mkdir -p "$OUTDIR/compiled" "$OUTDIR/classes" "$OUTDIR/dex" "$OUTDIR/gen"

# ---------- Step 1: aapt2 compile resources ----------
echo ">>> [1/8] aapt2 compile..."
"$AAPT2" compile --dir "$TWADIR/res" -o "$OUTDIR/compiled/res.zip"

# ---------- Step 2: aapt2 link ----------
echo ">>> [2/8] aapt2 link..."
"$AAPT2" link \
    -o "$OUTDIR/base.apk" \
    -I "$PLATFORM_JAR" \
    --manifest "$TWADIR/AndroidManifest.xml" \
    --java "$OUTDIR/gen" \
    --auto-add-overlay "$OUTDIR/compiled/res.zip" \
    --target-sdk-version "$PLATFORM_VER" \
    --min-sdk-version 24 \
    --version-code 1 \
    --version-name 0.1.0

# ---------- Step 3: javac compile Java sources ----------
echo ">>> [3/8] javac..."
JAVA_FILES=$(find "$TWADIR/src" -name "*.java")
if [[ -z "$JAVA_FILES" ]]; then
    echo "ERROR: no .java files found under $TWADIR/src" >&2
    exit 1
fi
# shellcheck disable=SC2086
"$JAVAC" -source 17 -target 17 \
    -bootclasspath "$PLATFORM_JAR" \
    -d "$OUTDIR/classes" \
    $JAVA_FILES

# ---------- Step 4: d8 dex ----------
echo ">>> [4/8] d8..."
CLASS_FILES=$(find "$OUTDIR/classes" -name "*.class")
# shellcheck disable=SC2086
"$D8" --output "$OUTDIR/dex" \
    --lib "$PLATFORM_JAR" \
    $CLASS_FILES

# ---------- Step 5: add classes.dex into the APK ----------
echo ">>> [5/8] jar uf (inject classes.dex)..."
cp "$OUTDIR/base.apk" "$OUTDIR/unaligned.apk"
(
    cd "$OUTDIR/dex"
    "$JAR" uf ../unaligned.apk classes.dex
)

# ---------- Step 6: zipalign ----------
echo ">>> [6/8] zipalign..."
"$ZIPALIGN" -f -p 4 "$OUTDIR/unaligned.apk" "$OUTDIR/app.apk"

# ---------- Step 7: sign ----------
echo ">>> [7/8] sign..."
if [[ ! -f "$KEYSTORE" ]]; then
    echo "    generating debug keystore at $KEYSTORE"
    "$KEYTOOL" -genkeypair \
        -keystore "$KEYSTORE" \
        -storepass android \
        -keypass android \
        -alias androiddebugkey \
        -keyalg RSA \
        -keysize 2048 \
        -validity 10000 \
        -dname "CN=Android Debug,O=Android,C=US"
fi
"$APKSIGNER" sign \
    --ks "$KEYSTORE" \
    --ks-key-alias androiddebugkey \
    --ks-pass pass:android \
    --key-pass pass:android \
    "$OUTDIR/app.apk"
"$APKSIGNER" verify --verbose "$OUTDIR/app.apk"

# ---------- Step 8: report ----------
echo
echo ">>> [8/8] done."
echo "=== BUILD SUCCESS ==="
ls -la "$OUTDIR/app.apk"
echo "Install: adb install $OUTDIR/app.apk"
