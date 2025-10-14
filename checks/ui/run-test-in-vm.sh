#/usr/bin/env bash
set -euxo pipefail

cp -r $1 /tmp/testdir
chmod +w -R /tmp/testdir

cd /tmp/testdir
CALIGULA="$2" python3 test_ui.py