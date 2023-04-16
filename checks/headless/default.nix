{ lib, caligula, runCommand }:
runCommand "caligula-headless-test" {
  buildInputs = [ caligula ];
  isoInnerHash = "3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986";
  meta.timeout = 10;
} ''
  caligula burn ${./input.iso.gz} \
    --force \
    -o ./out.iso \
    --hash $isoInnerHash \
    --hash-of raw \
    --compression auto 

  diff ${./expected.iso} ./out.iso

  echo 1 > $out
''
