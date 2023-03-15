{ lib, caligula, runCommand }:
runCommand "caligula-smoke-test-simple" {
  buildInputs = [ caligula ];
  meta.timeout = 10;
} ''
  caligula -h > $out
''
