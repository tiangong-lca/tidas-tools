import csv
import json

from tidas_tools.import_lca.cli import main
from tidas_tools.import_lca.mapping_csv import MAPPING_CSV_COLUMNS

FLOW_ID = "11111111-1111-4111-8111-111111111111"
PROCESS_ID = "22222222-2222-4222-8222-222222222222"


def test_openlca_jsonld_import_writes_expert_mapping_csv(tmp_path):
    source_dir = tmp_path / "jsonld"
    source_dir.mkdir()
    (source_dir / "flow.json").write_text(
        json.dumps(
            {
                "@type": "Flow",
                "@id": FLOW_ID,
                "name": "carbon dioxide",
                "category": "Elementary flows/air",
                "cas": "124-38-9",
                "formula": "CO2",
                "flowType": "ELEMENTARY_FLOW",
            }
        ),
        encoding="utf-8",
    )
    (source_dir / "process.json").write_text(
        json.dumps(
            {
                "@type": "Process",
                "@id": PROCESS_ID,
                "name": "Example process",
                "category": "Manufacturing/Test category",
                "location": "United States of America (the)",
                "exchanges": [
                    {
                        "internalId": 1,
                        "flow": {"@id": FLOW_ID, "name": "carbon dioxide"},
                        "amount": "__AMOUNT__",
                        "isInput": False,
                        "isQuantitativeReference": True,
                    }
                ],
            }
        ).replace('"__AMOUNT__"', "0.123456789012345678"),
        encoding="utf-8",
    )
    output_dir = tmp_path / "out"

    status = main(
        [
            "--input",
            str(source_dir),
            "--output-dir",
            str(output_dir),
            "--from-format",
            "openlca-jsonld",
        ]
    )

    rows = _mapping_rows(output_dir)
    report = json.loads(
        (output_dir / "conversion-report.json").read_text(encoding="utf-8")
    )

    assert status == 0
    assert rows
    assert report["target"]["mapping_csv"] == str(output_dir / "mapping.csv")
    assert report["target"]["mapping_csv_rows"] == len(rows)
    assert rows[0].keys() == dict.fromkeys(MAPPING_CSV_COLUMNS).keys()
    assert _has_row(
        rows,
        source_format="openlca-jsonld",
        target_field_name="CASNumber",
        target_value="124-38-9",
        mapping_status="formal",
        mapping_category="cas",
    )
    assert _has_row(
        rows,
        target_field_name="meanAmount",
        target_value="0.123456789012345678",
        mapping_status="formal",
        mapping_category="amount",
    )
    assert _has_row(
        rows,
        mapping_status="trace_only",
        mapping_category="classification",
        trace_marker="TIDAS_IMPORT_TRACE_V1",
    )
    assert _has_row(rows, mapping_status="placeholder", needs_review="TRUE")


def test_ecospold1_import_writes_same_expert_mapping_csv_schema(tmp_path):
    source = tmp_path / "dataset.xml"
    source.write_text(
        """<?xml version="1.0" encoding="UTF-8"?>
<ecoSpold xmlns="http://www.EcoInvent.org/EcoSpold01">
  <dataset>
    <metaInformation>
      <processInformation>
        <referenceFunction name="market for enriched steel" category="metals" subCategory="steel" />
        <geography location="CH" text="Swiss market boundary" />
        <timePeriod startDate="2020-01-01" endDate="2023-12-31" />
      </processInformation>
    </metaInformation>
    <flowData>
      <exchange number="1" name="enriched steel" unit="kg" amount="1" outputGroup="0" />
      <exchange number="2" name="carbon dioxide" CASNumber="124-38-9" formula="CO2" unit="kg" amount="2.5000000000000001" outputGroup="4" category="air" subCategory="unspecified" />
    </flowData>
  </dataset>
</ecoSpold>
""",
        encoding="utf-8",
    )
    output_dir = tmp_path / "out"

    status = main(["--input", str(source), "--output-dir", str(output_dir)])

    rows = _mapping_rows(output_dir)

    assert status == 0
    assert rows[0].keys() == dict.fromkeys(MAPPING_CSV_COLUMNS).keys()
    assert _has_row(
        rows,
        source_format="ecospold1",
        target_field_name="meanAmount",
        target_value="2.5000000000000001",
        source_exchange_id="2",
        mapping_status="formal",
        mapping_category="amount",
    )
    assert _has_row(
        rows,
        source_format="ecospold1",
        mapping_status="trace_only",
        mapping_category="classification",
    )
    assert _has_row(rows, source_format="ecospold1", mapping_status="placeholder")


def test_ecospold2_import_writes_same_expert_mapping_csv_schema(tmp_path):
    source = tmp_path / "activity.spold"
    source.write_text(
        f"""<?xml version="1.0" encoding="UTF-8"?>
<ecoSpold xmlns="http://www.EcoInvent.org/EcoSpold02">
  <activityDataset>
    <activityDescription>
      <activity id="{PROCESS_ID}">
        <activityName xml:lang="en">market for test product</activityName>
      </activity>
      <classification classificationId="activity-class">
        <classificationSystem xml:lang="en">EcoSpold activity classes</classificationSystem>
        <classificationValue xml:lang="en">energy systems</classificationValue>
      </classification>
      <timePeriod>
        <startDate>2024-01-01</startDate>
      </timePeriod>
      <geography>
        <shortname xml:lang="en">CH</shortname>
      </geography>
    </activityDescription>
    <flowData>
      <intermediateExchange id="{FLOW_ID}" amount="1">
        <name xml:lang="en">test product</name>
        <unitName xml:lang="en">kg</unitName>
        <outputGroup>0</outputGroup>
      </intermediateExchange>
      <elementaryExchange id="44444444-4444-4444-8444-444444444444" amount="1.5">
        <name xml:lang="en">carbon dioxide</name>
        <CASNumber>124-38-9</CASNumber>
        <unitName xml:lang="en">kg</unitName>
        <compartment>
          <compartment xml:lang="en">air</compartment>
          <subcompartment xml:lang="en">low population density</subcompartment>
        </compartment>
        <outputGroup>4</outputGroup>
      </elementaryExchange>
    </flowData>
  </activityDataset>
</ecoSpold>
""",
        encoding="utf-8",
    )
    output_dir = tmp_path / "out"

    status = main(["--input", str(source), "--output-dir", str(output_dir)])

    rows = _mapping_rows(output_dir)

    assert status == 0
    assert rows[0].keys() == dict.fromkeys(MAPPING_CSV_COLUMNS).keys()
    assert _has_row(
        rows,
        source_format="ecospold2",
        target_field_name="CASNumber",
        target_value="124-38-9",
        mapping_status="formal",
        mapping_category="cas",
    )
    assert _has_row(
        rows,
        source_format="ecospold2",
        mapping_status="trace_only",
        mapping_category="classification",
    )
    assert _has_row(rows, source_format="ecospold2", mapping_status="placeholder")


def _mapping_rows(output_dir):
    path = output_dir / "mapping.csv"
    assert path.is_file()
    with path.open(newline="", encoding="utf-8") as stream:
        return list(csv.DictReader(stream))


def _has_row(rows, **expected):
    return any(
        all(row.get(key) == value for key, value in expected.items()) for row in rows
    )
