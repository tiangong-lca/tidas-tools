import argparse
import json
import os
import shutil
import sys

import xmltodict


def convert_format(data, to_xml=True):
    """Convert between JSON and XML formats.

    Args:
        data: The input data string (JSON or XML)
        to_xml: If True, convert JSON to XML; if False, convert XML to JSON

    Returns:
        Converted data in the target format
    """
    if to_xml:
        # JSON to XML
        return xmltodict.unparse(
            json.loads(data) if isinstance(data, str) else data, pretty=True
        )
    else:
        # XML to JSON
        return xmltodict.parse(data)


def convert_directory(input_dir, output_dir, to_xml=True):
    data_dir = os.path.join(output_dir, "data")  # Make data a top-level directory
    os.makedirs(data_dir, exist_ok=True)

    for root, dirs, files in os.walk(input_dir):
        rel_dir = os.path.relpath(root, input_dir)
        target_dir = os.path.join(data_dir, rel_dir)  # Put subdirectories under data
        os.makedirs(target_dir, exist_ok=True)
        for file in files:
            source_file = os.path.join(root, file)

            # Determine if file should be processed based on extension
            process_file = False
            if to_xml and file.lower().endswith(".json"):
                # JSON to XML (default mode)
                target_extension = ".xml"
                process_file = True
            elif not to_xml and file.lower().endswith(".xml"):
                # XML to JSON
                target_extension = ".json"
                process_file = True

            if process_file:
                target_file = os.path.join(
                    target_dir, os.path.splitext(file)[0] + target_extension
                )
                try:
                    with open(source_file, "r", encoding="utf-8") as f:
                        data = f.read()

                    result = convert_format(data, to_xml=to_xml)

                    with open(target_file, "w", encoding="utf-8") as f:
                        if to_xml:
                            # JSON to XML - write string directly
                            f.write(result)
                        else:
                            # XML to JSON - format as JSON
                            json.dump(result, f, indent=2, ensure_ascii=False)

                    print(f"Converted: {source_file} -> {target_file}")
                except Exception as e:
                    print(f"Error converting {source_file}: {e}", file=sys.stderr)
            else:
                target_file = os.path.join(target_dir, file)
                try:
                    shutil.copy2(source_file, target_file)
                    print(f"Copied: {source_file} -> {target_file}")
                except Exception as e:
                    print(f"Error copying {source_file}: {e}", file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description="TIDAS and eILCD format converter.")
    parser.add_argument(
        "--input_dir",
        "-i",
        type=str,
        help="Input directory containing files to process",
    )
    parser.add_argument(
        "--output_dir",
        "-o",
        type=str,
        help="Output directory to store the converted files",
    )
    format_group = parser.add_mutually_exclusive_group()
    format_group.add_argument(
        "--to-eilcd",
        action="store_true",
        default=True,
        help="Convert JSON files to XML format (default)",
    )
    format_group.add_argument(
        "--to-tidas",
        action="store_true",
        dest="to_json",
        help="Convert XML files to JSON format",
    )

    args = parser.parse_args()

    if not os.path.isdir(args.input_dir):
        print(
            f"Error: Input directory '{args.input_dir}' does not exist or is not a directory.",
            file=sys.stderr,
        )
        sys.exit(1)

    os.makedirs(args.output_dir, exist_ok=True)
    to_xml = not args.to_json
    convert_directory(
        input_dir=args.input_dir, output_dir=args.output_dir, to_xml=to_xml
    )

    if to_xml:
        eilcd_dir = os.path.join(os.path.dirname(__file__), "eilcd")
        for item in os.listdir(eilcd_dir):
            item_path = os.path.join(eilcd_dir, item)
            if os.path.isdir(item_path):
                dest_path = os.path.join(args.output_dir, item)
                shutil.copytree(item_path, dest_path)
        print("Conversion from TIDAS to eILCD complete.")
    else:
        tidas_dir = os.path.join(os.path.dirname(__file__), "tidas")
        for item in os.listdir(tidas_dir):
            item_path = os.path.join(tidas_dir, item)
            if os.path.isdir(item_path):
                dest_path = os.path.join(args.output_dir, item)
                shutil.copytree(item_path, dest_path)
        print("Conversion from eILCD to TIDAS complete.")


if __name__ == "__main__":
    main()
