import argparse
import json
import logging
import os
import shutil
import sys

import xmltodict

logging.basicConfig(
    filename="tidas_convert.log",
    level=logging.INFO,
    format="%(asctime)s - %(message)s",
    datefmt="%d-%b-%y %H:%M:%S",
)

GREEN = "\033[92m"
RED = "\033[91m"
RESET = "\033[0m"


def convert_format(data, to_xml=True):
    """Convert between JSON and XML formats.

    Args:
        data: The input data string (JSON or XML)
        to_xml: If True, convert JSON to XML; if False, convert XML to JSON

    Returns:
        Converted data in the target format
    """
    try:
        if to_xml:
            # JSON to XML
            return xmltodict.unparse(
                json.loads(data) if isinstance(data, str) else data, pretty=True
            )
        else:
            # XML to JSON
            return xmltodict.parse(data)
    except Exception as e:
        logging.error(f"Format conversion error: {e}")
        raise


def convert_directory(input_dir, output_dir, to_xml=True):
    try:
        data_dir = os.path.join(output_dir, "data")  # Make data a top-level directory
        os.makedirs(data_dir, exist_ok=True)
        logging.info(f"Created output directory: {data_dir}")
    except Exception as e:
        error_msg = f"Failed to create output directory: {e}"
        print(f"{RED}Error: {error_msg}{RESET}", file=sys.stderr)
        logging.error(error_msg)
        return

    for root, dirs, files in os.walk(input_dir):
        try:
            rel_dir = os.path.relpath(root, input_dir)
            target_dir = os.path.join(
                data_dir, rel_dir
            )  # Put subdirectories under data
            os.makedirs(target_dir, exist_ok=True)
        except Exception as e:
            error_msg = f"Failed to create directory {target_dir}: {e}"
            print(f"{RED}Error: {error_msg}{RESET}", file=sys.stderr)
            logging.error(error_msg)
            continue

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

                    success_msg = f"Converted: {source_file} -> {target_file}"
                    print(f"{GREEN}{success_msg}{RESET}")
                    logging.info(success_msg)
                except Exception as e:
                    error_msg = f"Error converting {source_file}: {e}"
                    print(f"{RED}{error_msg}{RESET}", file=sys.stderr)
                    logging.error(error_msg)
            else:
                target_file = os.path.join(target_dir, file)
                try:
                    shutil.copy2(source_file, target_file)
                    copy_msg = f"Copied: {source_file} -> {target_file}"
                    print(f"{GREEN}{copy_msg}{RESET}")
                    logging.info(copy_msg)
                except Exception as e:
                    error_msg = f"Error copying {source_file}: {e}"
                    print(f"{RED}{error_msg}{RESET}", file=sys.stderr)
                    logging.error(error_msg)


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

    try:
        args = parser.parse_args()

        if not args.input_dir or not args.output_dir:
            error_msg = "Input and output directories must be specified"
            print(f"{RED}Error: {error_msg}{RESET}", file=sys.stderr)
            logging.error(error_msg)
            parser.print_help()
            sys.exit(1)

        if not os.path.isdir(args.input_dir):
            error_msg = f"Input directory '{args.input_dir}' does not exist or is not a directory."
            print(f"{RED}Error: {error_msg}{RESET}", file=sys.stderr)
            logging.error(error_msg)
            sys.exit(1)

        os.makedirs(args.output_dir, exist_ok=True)
        to_xml = not args.to_json
        logging.info(f"Starting conversion: to_xml={to_xml}")
        convert_directory(
            input_dir=args.input_dir, output_dir=args.output_dir, to_xml=to_xml
        )

        try:
            if to_xml:
                eilcd_dir = os.path.join(os.path.dirname(__file__), "eilcd")
                for item in os.listdir(eilcd_dir):
                    item_path = os.path.join(eilcd_dir, item)
                    if os.path.isdir(item_path):
                        dest_path = os.path.join(args.output_dir, item)
                        if os.path.exists(dest_path):
                            print(
                                f"{GREEN}Directory {dest_path} already exists, merging contents...{RESET}"
                            )
                            logging.info(
                                f"Directory {dest_path} already exists, merging contents"
                            )
                            # Copy directory contents instead of the entire directory
                            for sub_item in os.listdir(item_path):
                                sub_src = os.path.join(item_path, sub_item)
                                sub_dst = os.path.join(dest_path, sub_item)
                                if os.path.isdir(sub_src):
                                    if os.path.exists(sub_dst):
                                        shutil.rmtree(sub_dst)
                                    shutil.copytree(sub_src, sub_dst)
                                else:
                                    shutil.copy2(sub_src, sub_dst)
                        else:
                            shutil.copytree(item_path, dest_path)
                complete_msg = "Conversion from TIDAS to eILCD complete."
                print(f"{GREEN}{complete_msg}{RESET}")
                logging.info(complete_msg)
            else:
                tidas_dir = os.path.join(os.path.dirname(__file__), "tidas")
                for item in os.listdir(tidas_dir):
                    item_path = os.path.join(tidas_dir, item)
                    if os.path.isdir(item_path):
                        dest_path = os.path.join(args.output_dir, item)
                        if os.path.exists(dest_path):
                            print(
                                f"{GREEN}Directory {dest_path} already exists, merging contents...{RESET}"
                            )
                            logging.info(
                                f"Directory {dest_path} already exists, merging contents"
                            )
                            # Copy directory contents instead of the entire directory
                            for sub_item in os.listdir(item_path):
                                sub_src = os.path.join(item_path, sub_item)
                                sub_dst = os.path.join(dest_path, sub_item)
                                if os.path.isdir(sub_src):
                                    if os.path.exists(sub_dst):
                                        shutil.rmtree(sub_dst)
                                    shutil.copytree(sub_src, sub_dst)
                                else:
                                    shutil.copy2(sub_src, sub_dst)
                        else:
                            shutil.copytree(item_path, dest_path)
                complete_msg = "Conversion from eILCD to TIDAS complete."
                print(f"{GREEN}{complete_msg}{RESET}")
                logging.info(complete_msg)
        except Exception as e:
            error_msg = f"Error copying template files: {e}"
            print(f"{RED}{error_msg}{RESET}", file=sys.stderr)
            logging.error(error_msg)
    except Exception as e:
        error_msg = f"Unexpected error: {e}"
        print(f"{RED}{error_msg}{RESET}", file=sys.stderr)
        logging.error(error_msg)
        sys.exit(1)


if __name__ == "__main__":
    main()
