"""Generate synthetic fixtures and report actual package/semantic differences.
Usage: python probe.py PATH_TO_NATIVE_BINARY OUTPUT_DIRECTORY
No Word/EPUB rendering, schema conformance or browser execution is claimed.
"""
import base64
import hashlib
import json
import pathlib
import posixpath
import subprocess
import sys
import xml.etree.ElementTree as ET
import zipfile

sys.stdout.reconfigure(encoding="utf-8")

exe, root = pathlib.Path(sys.argv[1]).resolve(), pathlib.Path(sys.argv[2]).resolve()
root.mkdir(parents=True, exist_ok=True)
W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG = "http://schemas.openxmlformats.org/package/2006/relationships"
PNG = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=")
text = "中文与 emoji 🌌：白塔 & 海港"
doc = f'''<w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:x="urn:cap07e:unknown"><w:body>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>第一章</w:t></w:r></w:p>
<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>中文与 emoji 🌌：白塔 &amp; 海港</w:t></w:r><w:hyperlink r:id="link"><w:r><w:t>参考链接</w:t></w:r></w:hyperlink><w:r><w:footnoteReference w:id="1"/></w:r></w:p>
<w:p><w:commentRangeStart w:id="0"/><w:r><w:t>被批注正文</w:t></w:r><w:commentRangeEnd w:id="0"/><w:r><w:commentReference w:id="0"/></w:r></w:p>
<w:p><w:r><w:drawing><wp:inline xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><wp:extent cx="9525" cy="9525"/><wp:docPr id="1" name="pixel"/><a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:nvPicPr><pic:cNvPr id="1" name="pixel"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="img"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="9525" cy="9525"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>
<x:future x:flag="preserve">未知字段内容</x:future><w:sectPr/></w:body></w:document>'''
rels = f'''<Relationships xmlns="{PKG}">
<Relationship Id="link" Type="{R}/hyperlink" Target="https://example.invalid/reference" TargetMode="External"/>
<Relationship Id="img" Type="{R}/image" Target="media/pixel.png"/>
<Relationship Id="styles" Type="{R}/styles" Target="styles.xml"/>
<Relationship Id="footnotes" Type="{R}/footnotes" Target="footnotes.xml"/>
<Relationship Id="comments" Type="{R}/comments" Target="comments.xml"/>
</Relationships>'''
parts = {
    "[Content_Types].xml": '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/>' + ''.join(f'<Override PartName="/word/{part}.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.{mime}+xml"/>' for part, mime in [("document", "document.main"), ("styles", "styles"), ("footnotes", "footnotes"), ("comments", "comments")]) + '</Types>',
    "_rels/.rels": f'<Relationships xmlns="{PKG}"><Relationship Id="doc" Type="{R}/officeDocument" Target="word/document.xml"/></Relationships>',
    "word/document.xml": doc,
    "word/_rels/document.xml.rels": rels,
    "word/styles.xml": f'<w:styles xmlns:w="{W}"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style></w:styles>',
    "word/footnotes.xml": f'<w:footnotes xmlns:w="{W}"><w:footnote w:id="1"><w:p><w:r><w:t>脚注内容 🌌</w:t></w:r></w:p></w:footnote></w:footnotes>',
    "word/comments.xml": f'<w:comments xmlns:w="{W}"><w:comment w:id="0" w:author="研究" w:date="2026-09-26T00:00:00Z"><w:p><w:r><w:t>批注内容</w:t></w:r></w:p></w:comment></w:comments>',
    "word/media/pixel.png": PNG,
    "customXml/item1.xml": '<future>未建模扩展</future>',
}

def package(path, entries):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for name, value in entries.items():
            info = zipfile.ZipInfo(name, (2026, 9, 26, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, value.encode("utf-8") if isinstance(value, str) else value)

def run(label, args, expected_success):
    p = subprocess.run([str(exe), *map(str, args)], capture_output=True, text=True, encoding="utf-8", timeout=30)
    return {"case": label, "expected_success": expected_success, "actual_exit": p.returncode,
            "expectation_matched": (p.returncode == 0) == expected_success, "metrics_and_errors": p.stderr.strip()}

original, roundtrip = root / "fixture.docx", root / "roundtrip.docx"
for name, value in parts.items():
    if name.endswith('.xml') or name.endswith('.rels'):
        ET.fromstring(value)
package(original, parts)
rows = [run("docx_roundtrip", ["docx", original, roundtrip], True)]
simple = root / 'simple.docx'
simple_out = root / 'simple-out.docx'
package(simple, {**parts, 'word/document.xml': f'<w:document xmlns:w="{W}"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>中文 🌌</w:t></w:r></w:p><w:sectPr/></w:body></w:document>'})
rows.append(run('simple_docx_roundtrip', ['docx', simple, simple_out], True))
simple_diff = {}
if simple_out.exists():
    with zipfile.ZipFile(simple_out) as z:
        simple_xml = ET.fromstring(z.read('word/document.xml'))
    simple_diff = {'unicode_text_preserved': '中文 🌌' in ''.join(simple_xml.itertext()),
                   'heading_style_reference_preserved': any(e.get(f'{{{W}}}val') == 'Heading1' for e in simple_xml.iter(f'{{{W}}}pStyle')),
                   'bold_element_preserved': simple_xml.find(f'.//{{{W}}}b') is not None}
diff = {}
if roundtrip.exists():
    with zipfile.ZipFile(roundtrip) as z:
        output_parts = {name: z.read(name) for name in z.namelist()}
    parse_error = None
    try:
        output_doc = ET.fromstring(output_parts["word/document.xml"])
    except ET.ParseError as error:
        parse_error = str(error)
        output_doc = ET.Element('invalid-output')
    words = ''.join(output_doc.itertext())
    xml_parts = '\n'.join(data.decode('utf-8', errors='replace') for name, data in output_parts.items() if name.endswith('.xml'))
    diff = {
        "output_xml_parse_error": parse_error,
        "unicode_text_preserved": text in words,
        "heading_style_reference_preserved": any(e.get(f"{{{W}}}val") == "Heading1" for e in output_doc.iter(f"{{{W}}}pStyle")),
        "bold_element_preserved": output_doc.find(f".//{{{W}}}b") is not None,
        "hyperlink_target_preserved": any(b'https://example.invalid/reference' in data for name, data in output_parts.items() if name.endswith('.rels')),
        "image_bytes_preserved": PNG in output_parts.values(),
        "footnote_text_preserved": "脚注内容 🌌" in xml_parts,
        "comment_text_preserved": "批注内容" in xml_parts,
        "unknown_element_preserved": output_doc.find('.//{urn:cap07e:unknown}future') is not None,
        "unknown_part_preserved": output_parts.get('customXml/item1.xml') == parts['customXml/item1.xml'].encode(),
        "removed_part_names": sorted(set(parts) - set(output_parts)),
        "byte_identical": original.read_bytes() == roundtrip.read_bytes(),
    }
    if parse_error:
        for key in ['unicode_text_preserved', 'heading_style_reference_preserved', 'bold_element_preserved', 'unknown_element_preserved']:
            diff[key] = None

for label, changed in [
    ('malformed_xml', {**parts, 'word/document.xml': '<w:document><broken>'}),
    ('dtd', {**parts, 'word/document.xml': '<!DOCTYPE x [<!ENTITY e "x">]><x/>'}),
    ('relative_escape', {**parts, '../escape.xml': '<x/>'}),
    ('high_ratio', {**parts, 'padding.xml': '<x>' + 'A' * 300000 + '</x>'}),
    ('entry_limit', {**parts, 'large.xml': 'x' * 4194305}),
    ('macro_part', {**parts, 'word/vbaProject.bin': b'macro placeholder'}),
]:
    path = root / (label + '.docx')
    package(path, changed)
    rows.append(run(label, ['docx', path, root / (label + '-out.docx')], False))
bad = root / 'truncated.docx'
bad.write_bytes(original.read_bytes()[:100])
rows.append(run('truncated_zip', ['docx', bad, root / 'truncated-out.docx'], False))
huge = root / 'package_limit.docx'
huge.write_bytes(b'0' * 1048577)
rows.append(run('package_limit', ['docx', huge, root / 'package-out.docx'], False))

xhtml = '<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>第一章</title></head><body><h1 id="chapter">第一章</h1><p>中文与 emoji 🌌：白塔 &amp; 海港 <a href="#note" epub:type="noteref">脚注</a></p><aside id="note" epub:type="footnote">脚注内容 🌌</aside><img src="images/pixel.png" alt="像素"/><p data-future="unknown">扩展属性</p><!-- 批注只是 XML 注释，不是编辑器讨论模型 --></body></html>'
chapter, png, epub = root / 'chapter.xhtml', root / 'pixel.png', root / 'fixture.epub'
chapter.write_text(xhtml, encoding='utf-8')
png.write_bytes(PNG)
rows.append(run('epub_export', ['epub', chapter, png, epub], True))
epub_diff = {}
if epub.exists():
    with zipfile.ZipFile(epub) as z:
        names = z.namelist()
        chapter_name = next(name for name in names if name.endswith('/chapter.xhtml') or name == 'chapter.xhtml')
        image_name = posixpath.normpath(posixpath.join(posixpath.dirname(chapter_name), 'images/pixel.png'))
        epub_diff = {'xhtml_bytes_preserved': z.read(chapter_name) == chapter.read_bytes(),
                     'relative_image_resolved': image_name in names,
                     'image_bytes_preserved': image_name in names and z.read(image_name) == PNG,
                     'mimetype_first_stored': names[0] == 'mimetype' and z.getinfo('mimetype').compress_type == zipfile.ZIP_STORED,
                     'opf_present': any(name.endswith('.opf') for name in names),
                     'nav_present': any(name.endswith('nav.xhtml') for name in names)}
report = {'cases': rows, 'simple_docx_checks': simple_diff, 'docx_semantic_checks': diff, 'epub_package_checks': epub_diff,
          'fixture_sha256': hashlib.sha256(original.read_bytes()).hexdigest(),
          'scope': 'synthetic package checks only; no rendering, epubcheck, Word validation, EPUB import, or browser runtime',
          'heap_measurement': 'peak live Rust global allocator bytes for whole process; not RSS or an enforced allocation budget'}
(root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
print(json.dumps(report, ensure_ascii=False, indent=2))
if not all(row['expectation_matched'] for row in rows):
    sys.exit(1)
