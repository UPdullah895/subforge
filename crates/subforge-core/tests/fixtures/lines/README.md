# Translation line pipeline parity fixtures

Frozen outputs of the Python helper `scripts/subtitle_lines.py` (pysubs2 1.9.0),
generated once from the repository root. The `.expected` files are never
regenerated from Rust.

```sh
PY=~/.local/share/subforge/venv/bin/python
L=crates/subforge-core/tests/fixtures/lines
A=crates/subforge-core/tests/fixtures/subtitles

$PY scripts/subtitle_lines.py extract $A/arabic_full.ass > $L/arabic_full.extract.expected
$PY scripts/subtitle_lines.py extract $A/basic.srt       > $L/basic.extract.expected
$PY scripts/subtitle_lines.py extract $L/dash.srt        > $L/dash.extract.expected

$PY scripts/subtitle_lines.py rebuild $A/arabic_full.ass $L/translations_arabic_full.json $L/arabic_full.rebuilt.expected.ass --rtl-fix
$PY scripts/subtitle_lines.py rebuild $A/basic.srt       $L/translations_basic.json       $L/basic.rebuilt.expected.srt       --rtl-fix
$PY scripts/subtitle_lines.py rebuild $L/dash.srt        $L/translations_dash.json        $L/dash.rebuilt.expected.srt        --rtl-fix
```

`dash.srt` is local to this directory (dash-led dialogue source). The rebuilt
files keep a real subtitle extension because pysubs2 picks the output format
from it.

Known differences the parity tests ignore: Python also translates the ASS
`Comment:` cue and `extract` sends it for translation (Rust leaves it alone and
extracts empty text); pysubs2 rewrites the ASS header/section order, the
non-standard `Dialogue:0,` spacing and trailing whitespace, which the Rust
writer preserves.
