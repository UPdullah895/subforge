import sys, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).parent))
import subtitle_lines as sl


def test_plain_text_strips_tags_and_joins_breaks():
    assert sl.plain_text(r"{\i1}An encounter swallows{\i0}\N{\i1}me up{\i0}") == "An encounter swallows me up"
    assert sl.plain_text(r"{\an8}Hello\Nthere") == "Hello there"
    assert sl.plain_text(r"{\p1}m 0 0 l 10 10{\p0}") == ""


def test_reflow_keeps_italics_and_position():
    out = sl.reflow(r"{\an8}{\i1}Hello{\i0}\N{\i1}there{\i0}", "مرحبا بكم جميعا")
    assert out.startswith(r"{\an8}")
    assert r"{\i1}" in out and r"{\i0}" in out
    assert out.count("\n") == 1


def test_reflow_dash_dialogue():
    out = sl.reflow(r"- Where?\N- Home.", "- أين؟ - إلى البيت.")
    assert out.split("\n") == ["- أين؟", "- إلى البيت."]


def test_reflow_wraps_long_single_line():
    out = sl.reflow("Short", "كلمة " * 20)
    assert "\n" in out and all(len(p) <= 60 for p in out.split("\n"))


def test_reflow_plain_not_italic():
    assert sl.reflow("Hello", "مرحبا") == "مرحبا"


def test_arabic_punctuation():
    assert sl.arabic_punctuation("ماذا, هل؟ لا; نعم?") == "ماذا\u060c هل؟ لا\u061b نعم\u061f"
    assert sl.arabic_punctuation("مبلغ 1,000 ريال") == "مبلغ 1,000 ريال"
    assert sl.arabic_punctuation("What? Yes, no") == "What? Yes, no"
