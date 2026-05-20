"""Bridge from generated TIDAS JSON packages to existing eILCD XML output."""

from __future__ import annotations

import os
import shutil
from pathlib import Path

from ...convert import convert_directory


def write_ilcd_from_tidas(tidas_dir: str | Path, ilcd_dir: str | Path) -> None:
    """Reuse the existing TIDAS -> eILCD directory conversion path."""

    target = Path(ilcd_dir)
    convert_directory(str(tidas_dir), str(target), to_xml=True)
    _copy_eilcd_assets(target)


def _copy_eilcd_assets(output_dir: Path) -> None:
    eilcd_dir = Path(__file__).resolve().parents[2] / "eilcd"
    for item in os.listdir(eilcd_dir):
        item_path = eilcd_dir / item
        if not item_path.is_dir():
            continue
        dest_path = output_dir / item
        if dest_path.exists():
            for sub_item in os.listdir(item_path):
                sub_src = item_path / sub_item
                sub_dst = dest_path / sub_item
                if sub_src.is_dir():
                    if sub_dst.exists():
                        shutil.rmtree(sub_dst)
                    shutil.copytree(sub_src, sub_dst)
                else:
                    shutil.copy2(sub_src, sub_dst)
        else:
            shutil.copytree(item_path, dest_path)
