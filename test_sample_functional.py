import glob
import json
import os

from src.tidas_tools import category_validate


def test_example():
    base_dir = "tests/test_data"
    for category in os.listdir(base_dir):
        category_path = os.path.join(base_dir, category)
        if os.path.isdir(category_path):
            data_list = []
            for file in glob.glob(os.path.join(category_path, "*.json")):
                with open(file, "r", encoding="utf-8") as f:
                    data_list.append(json.load(f))
                category_validate(data_list, category)


test_example()
