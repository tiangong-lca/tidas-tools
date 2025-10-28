from tidas_tools.validate import category_validate


def test_category_validate(tmp_path):
    sources_dir = tmp_path / "sources"
    sources_dir.mkdir()

    category_validate(str(sources_dir), "sources")
