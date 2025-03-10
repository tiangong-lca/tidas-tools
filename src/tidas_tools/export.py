import argparse
import logging
import os
import shutil
import sys
from pathlib import Path

import boto3
import psycopg2
import xmltodict
from dotenv import load_dotenv
from tqdm import tqdm


def setup_logging(verbose):
    """Configure logging"""
    log_level = logging.DEBUG if verbose else logging.INFO

    handlers = [
        logging.FileHandler("export_xml.log", mode="w"),
        logging.StreamHandler(sys.stdout),
    ]

    logging.basicConfig(
        level=log_level,
        format="%(asctime)s:%(levelname)s:%(message)s",
        handlers=handlers,
    )


def zip_folder(folder_path, output_path):
    """Compress a folder and return the compressed file path"""
    shutil.make_archive(output_path, "zip", folder_path)
    logging.info(f"Created compressed file: {output_path}.zip")
    return f"{output_path}.zip"


def process_record(input_dir, category, record):
    """Process a single record and save as XML"""
    id, json_ordered, version = record
    xml_data = xmltodict.unparse(json_ordered, pretty=True)

    category_dir = Path(input_dir) / category
    category_dir.mkdir(exist_ok=True, parents=True)

    xml_path = category_dir / f"{id}_{version}.xml"
    with open(xml_path, "w") as f:
        f.write(xml_data)


def process_common_record(input_dir, record):
    """Process a common record and save as XML"""
    id, json_ordered = record
    xml_data = xmltodict.unparse(json_ordered, pretty=True)

    input_path = Path(input_dir)
    input_path.mkdir(exist_ok=True, parents=True)

    xml_path = input_path / f"{id}.xml"
    with open(xml_path, "w") as f:
        f.write(xml_data)


def export_common_records(conn, input_dir):
    """Export common records"""
    logging.info("Exporting common records...")
    cursor = conn.cursor()
    cursor.execute("SET statement_timeout = '600000';")
    cursor.execute("SELECT file_name, json_ordered FROM ilcd")

    # Get total record count for progress bar
    cursor2 = conn.cursor()
    cursor2.execute("SELECT COUNT(*) FROM ilcd")
    total = cursor2.fetchone()[0]
    cursor2.close()

    if total == 0:
        logging.info("No common records found")
        return

    records = cursor.fetchall()

    with tqdm(total=total, desc="Exporting common records") as pbar:
        for record in records:
            process_common_record(input_dir, record)
            pbar.update(1)

    cursor.close()
    logging.info(f"Exported {total} common records")


def export_category_records(conn, input_dir, categories):
    """Export records by category"""
    logging.info("Exporting category records...")

    # Create directories for all categories
    for category in categories:
        category_dir = Path(input_dir) / category
        category_dir.mkdir(exist_ok=True, parents=True)

    for category in categories:
        cursor = conn.cursor()
        cursor.execute(
            f"SELECT id, json_ordered, version FROM {category} WHERE state_code = 100"
        )

        batch_size = 500
        total_exported = 0

        while True:
            records = cursor.fetchmany(batch_size)
            if not records:
                break

            with tqdm(total=len(records), desc=f"Exporting {category}") as pbar:
                for record in records:
                    process_record(input_dir, category, record)
                    pbar.update(1)
                    total_exported += 1

        cursor.close()
        logging.info(f"Exported {total_exported} {category} records")


def download_external_docs(s3, bucket_name, input_dir):
    """Download external documents from S3"""
    logging.info("Downloading external documents...")

    try:
        paginator = s3.get_paginator("list_objects_v2")

        # Get total file count
        total_files = 0
        for page in paginator.paginate(Bucket=bucket_name):
            if "Contents" in page:
                total_files += len(page["Contents"])

        if total_files == 0:
            logging.info("No documents in bucket")
            return

        external_docs_dir = Path(input_dir) / "external_docs"
        external_docs_dir.mkdir(exist_ok=True, parents=True)

        with tqdm(total=total_files, desc="Downloading external documents") as pbar:
            for page in paginator.paginate(Bucket=bucket_name):
                if "Contents" in page:
                    for obj in page["Contents"]:
                        key = obj["Key"]
                        local_path = external_docs_dir / key
                        local_path.parent.mkdir(exist_ok=True, parents=True)

                        s3.download_file(bucket_name, key, str(local_path))
                        pbar.update(1)

        logging.info(f"Downloaded {total_files} external documents")

    except Exception as e:
        logging.error(f"Error downloading external documents: {str(e)}")
        raise


def parse_arguments():
    """Parse command line arguments"""
    parser = argparse.ArgumentParser(
        description="Export database records as XML files and create a ZIP archive. "
        "This tool connects to a database, exports records as XML, optionally downloads "
        "external documents from S3-compatible storage, and creates a ZIP archive containing "
        "all exported data."
    )

    parser.add_argument(
        "--input-dir",
        default="dist/tiangong",
        help="Input directory to store XML files",
    )
    parser.add_argument(
        "--output-zip",
        default="dist/tiangong",
        help="Output zip filename (without .zip extension)",
    )
    parser.add_argument(
        "--env-file", default=".env", help="Path to .env file with credentials"
    )
    parser.add_argument("--db-user", help="Database username")
    parser.add_argument("--db-password", help="Database password")
    parser.add_argument("--db-host", help="Database host")
    parser.add_argument("--db-port", help="Database port (default: 5432)")
    parser.add_argument("--db-name", help="Database name")
    parser.add_argument("--aws-region", help="AWS region")
    parser.add_argument("--aws-endpoint", help="AWS endpoint URL")
    parser.add_argument("--aws-access-key-id", help="AWS access key ID")
    parser.add_argument("--aws-secret-access-key", help="AWS secret access key")
    parser.add_argument("--aws-bucket", help="AWS bucket for external documents")
    parser.add_argument(
        "--verbose", "-v", action="store_true", help="Enable verbose logging"
    )
    parser.add_argument(
        "--skip-external-docs",
        action="store_true",
        help="Skip downloading external documents",
    )

    return parser.parse_args()


def main():
    """Main function to be imported by other scripts"""
    args = parse_arguments()

    # Set up logging
    setup_logging(args.verbose)

    # Load environment variables
    if os.path.exists(args.env_file):
        load_dotenv(args.env_file)

    # Merge CLI params and environment variables
    db_params = {
        "user": args.db_user or os.getenv("DB_USER"),
        "password": args.db_password or os.getenv("DB_PASSWORD"),
        "host": args.db_host or os.getenv("DB_HOST"),
        "port": args.db_port or os.getenv("DB_PORT") or "5432",
        "dbname": args.db_name or os.getenv("DB_NAME"),
    }

    # Validate required database parameters
    missing_db_params = [k for k, v in db_params.items() if not v and k != "port"]
    if missing_db_params:
        print(
            f"\033[91mError: Missing database parameters: {', '.join(missing_db_params)}\033[0m",
            file=sys.stderr,
        )
        return 1

    try:
        # Create input directory
        Path(args.input_dir).mkdir(exist_ok=True, parents=True)

        # Create output directory
        Path(args.output_zip).parent.mkdir(exist_ok=True, parents=True)

        # Connect to database
        logging.info("Connecting to database...")
        conn = psycopg2.connect(**db_params)
        logging.info("Database connection successful")

        # Export common records
        export_common_records(conn, args.input_dir)

        # Export category records
        categories = [
            "contacts",
            "flows",
            "flowproperties",
            "processes",
            "sources",
            "unitgroups",
            "lciamethods",
            "lifecyclemodels",
        ]
        export_category_records(conn, args.input_dir, categories)

        # Close database connection
        conn.close()
        logging.info("Database connection closed")

        # Download external documents if needed
        if not args.skip_external_docs:
            # Set up AWS parameters
            aws_params = {
                "region_name": args.aws_region or os.getenv("AWS_REGION"),
                "endpoint_url": args.aws_endpoint or os.getenv("AWS_ENDPOINT"),
                "aws_access_key_id": args.aws_access_key_id
                or os.getenv("AWS_ACCESS_KEY_ID"),
                "aws_secret_access_key": args.aws_secret_access_key
                or os.getenv("AWS_SECRET_ACCESS_KEY"),
            }
            bucket = args.aws_bucket or os.getenv("AWS_EXTERNAL_DOCS_BUCKET")

            # Validate AWS parameters
            if all(aws_params.values()) and bucket:
                # Configure S3 client
                s3 = boto3.client("s3", **aws_params)

                # Download external documents
                download_external_docs(s3, bucket, args.input_dir)
            else:
                logging.warning(
                    "Missing AWS parameters, skipping external document download"
                )

        # Compress output
        zip_file = zip_folder(args.input_dir, args.output_zip)

        print(f"\033[92mExport successfully completed!\033[0m")
        print(f"Output file: {zip_file}")
        return 0

    except psycopg2.Error as e:
        logging.error(f"Database error: {str(e)}")
        print(f"\033[91mDatabase error: {str(e)}\033[0m", file=sys.stderr)
        return 1
    except Exception as e:
        logging.error(f"Error: {str(e)}")
        print(f"\033[91mError: {str(e)}\033[0m", file=sys.stderr)
        return 1


if __name__ == "__main__":
    main()
