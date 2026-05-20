import json
import zipfile

from tidas_tools.import_lca.cli import main


def test_process_xlsx_minimal_import_can_write_valid_tidas_and_ilcd(tmp_path):
    source = tmp_path / "process.xlsx"
    write_minimal_process_xlsx(source)
    output_dir = tmp_path / "out"

    status = main(
        [
            "--input",
            str(source),
            "--output-dir",
            str(output_dir),
            "--target",
            "both",
        ]
    )

    report = json.loads(
        (output_dir / "conversion-report.json").read_text(encoding="utf-8")
    )

    assert status == 0
    assert report["source"]["detected_format"] == "openlca-process-xlsx"
    assert report["summary"]["flows"] == 3
    assert report["summary"]["processes"] == 1
    assert report["validation"]["tidas"]["ok"] is True
    assert report["validation"]["ilcd"]["ok"] is True
    assert _has_flow_property(output_dir / "tidas", "Amount in kg")


def write_minimal_process_xlsx(path):
    sheets = {
        "General information": [
            ["General information"],
            ["UUID", "22222222-2222-4222-8222-222222222222"],
            ["Name", "XLSX test process"],
            ["Description", "Minimal openLCA process workbook fixture"],
            [],
        ],
        "Flows": [
            ["UUID", "Name", "Category", "Type", "Reference flow property"],
            [
                "11111111-1111-4111-8111-111111111111",
                "test product",
                "products",
                "Product flow",
                "Mass",
            ],
            [
                "33333333-3333-4333-8333-333333333333",
                "test input",
                "materials",
                "Product flow",
                "Mass",
            ],
            [
                "44444444-4444-4444-8444-444444444444",
                "carbon dioxide",
                "air",
                "Elementary flow",
                "Mass",
            ],
        ],
        "Outputs": [
            ["Flow", "Category", "Amount", "Unit", "Is reference?"],
            ["test product", "products", "1", "kg", "true"],
            ["carbon dioxide", "air", "1.5", "kg", "false"],
        ],
        "Inputs": [
            ["Flow", "Category", "Amount", "Unit"],
            ["test input", "materials", "0.2", "kg"],
        ],
    }
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("[Content_Types].xml", _content_types())
        archive.writestr("xl/workbook.xml", _workbook_xml(sheets))
        archive.writestr("xl/_rels/workbook.xml.rels", _workbook_rels(len(sheets)))
        for index, rows in enumerate(sheets.values(), start=1):
            archive.writestr(f"xl/worksheets/sheet{index}.xml", _sheet_xml(rows))


def _content_types():
    return """<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
</Types>
"""


def _workbook_xml(sheets):
    items = []
    for index, name in enumerate(sheets, start=1):
        items.append(f'<sheet name="{name}" sheetId="{index}" r:id="rId{index}"/>')
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
  xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>{''.join(items)}</sheets>
</workbook>
"""


def _workbook_rels(count):
    items = []
    for index in range(1, count + 1):
        items.append(
            f'<Relationship Id="rId{index}" '
            'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" '
            f'Target="worksheets/sheet{index}.xml"/>'
        )
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  {''.join(items)}
</Relationships>
"""


def _sheet_xml(rows):
    row_items = []
    for row_index, row in enumerate(rows, start=1):
        cell_items = []
        for column_index, value in enumerate(row, start=1):
            cell_items.append(
                f'<c r="{_cell_ref(column_index, row_index)}" t="inlineStr">'
                f"<is><t>{value}</t></is></c>"
            )
        row_items.append(f'<row r="{row_index}">{"".join(cell_items)}</row>')
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>{''.join(row_items)}</sheetData>
</worksheet>
"""


def _cell_ref(column_index, row_index):
    letters = ""
    while column_index:
        column_index, remainder = divmod(column_index - 1, 26)
        letters = chr(ord("A") + remainder) + letters
    return f"{letters}{row_index}"


def _has_flow_property(tidas_dir, expected_name):
    for path in (tidas_dir / "flowproperties").glob("*.json"):
        data = json.loads(path.read_text(encoding="utf-8"))
        name = data["flowPropertyDataSet"]["flowPropertiesInformation"][
            "dataSetInformation"
        ]["common:name"]["#text"]
        if name == expected_name:
            return True
    return False
