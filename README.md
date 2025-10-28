# TianGong TIDAS Tools User Guide

[![PyPI](https://img.shields.io/pypi/v/tidas-tools.svg)][pypi status]
[![Python Version](https://img.shields.io/pypi/pyversions/tidas-tools)][pypi status]

[pypi status]: https://pypi.org/project/tidas-tools/

[English](https://github.com/tiangong-lca/tidas-tools/blob/main/README.md) | [中文](https://github.com/tiangong-lca/tidas-tools/blob/main/README_CN.md)

This toolkit is used for conversion and validation of TianGong TIDAS data formats.

---

## 1. Introduction

This toolkit contains two independent tools:

- **TIDAS and eILCD Data Format Conversion Tool**
- **TIDAS Data Validation Tool**

---

## 2. TIDAS and eILCD Data Format Conversion Tool Usage

### (1) Installation Instructions

```bash
# Install this toolkit
pip install tidas-tools
```

### (2) Tool Functionalities

This tool supports mutual conversion between the following two data formats:

- TIDAS data format → eILCD data format (default mode)
- eILCD data format → TIDAS data format

### (3) Command-line Arguments

| Argument | Short form | Description |
|----------|------------|-------------|
| `--help` | `-h` | Display help message |
| `--input-dir` | `-i` | Directory containing data files to be converted (note: this directory must directly contain the data files, not their parent directory) |
| `--output-dir` | `-o` | Output directory for converted data (the program will automatically generate the complete schema-compatible directory structure) |
| `--to-eilcd` | | Convert data from TIDAS format to eILCD format (default mode) |
| `--to-tidas` | | Convert data from eILCD format to TIDAS format |
| `--verbose` | `-v` | Enable verbose logging |

### (4) Usage Examples

```bash
# Convert TIDAS data to eILCD format
tidas-convert --input-dir <TIDAS_data_directory> --output-dir <eILCD_output_directory> --to-eilcd

# Convert eILCD data to TIDAS format
tidas-convert --input-dir <eILCD_data_directory> --output-dir <TIDAS_output_directory> --to-tidas
```

---

## 3. TIDAS Data Validation Tool Usage

### (1) Tool Functionalities

This tool validates whether TIDAS data complies with the specified format standards.

### (2) Command-line Arguments

| Argument | Short form | Description |
|----------|------------|-------------|
| `--help` | `-h` | Display help message |
| `--input-dir` | `-i` | Directory containing TIDAS data to validate (note: this directory must directly contain the data files, not their parent directory) |
| `--verbose` | `-v` | Enable verbose logging |

### (3) Usage Example

```bash
# Validate TIDAS data format
tidas-validate --input-dir <TIDAS_data_directory>
```

## 4. TIDAS Export Tool Documentation

### (1) Tool Functionalities

This tool exports data records in either TIDAS or eILCD format. It also optionally downloads supplementary files and bundles them into a final zip archive.

### (2) Command-line Arguments and Environment Variables

| Parameter                 | Short | Description                                     |
|---------------------------|-------|-------------------------------------------------|
| `--help`                  | `-h`  | Display help information                        |
| `--to-tidas`              | -     | Export data in TIDAS format (default)           |
| `--to-eilcd`              | None  | Export data in eILCD format                     |
| `--input-dir`             | `-i`  | Input directory containing files to export      |
| `--output-zip`            | `-z`  | Output path for the zip file                    |
| `--env-file`              | `-e`  | Path to .env file containing DB and AWS credentials|
| `--skip-external-docs`    |       | Skip downloading external supplementary files   |
| `--to-tidas`              |       | Export in TIDAS format (default option)         |
| `--to-eilcd`              |       | Export in eILCD format (mutually exclusive)     |
| `--db-user`               |       | Database username                               |
| `--db-password`           |       | Database password                               |
| `--db-host`               |       | Database host                                   |
| `--db-port`               |       | Database port (default: 5432)                   |
| `--db-name`               |       | Database name                                   |
| `--aws-access-key-id`     |       | AWS access key ID                               |
| `--aws-secret-access-key` |       | AWS secret access key                           |
| `--aws-region`            |       | AWS region                                      |
| `--verbose`               | `-v`  | Enable verbose logging                          |

Credentials can also be set via environment variables (defaults to the .env file in the current directory):

```env
DB_USER=
DB_PASSWORD=
DB_HOST=
DB_PORT=5432
DB_NAME=postgres
AWS_REGION=
AWS_ENDPOINT=
```

### (3) Usage Example

```bash
# Export records to TIDAS format and produce a ZIP archive.
tidas-export -i <TIDAS_input_directory> -z <TIDAS_ZIP_File> --to-tidas

# Export records to eILCD format without downloading supplementary files
tidas-export -z <eILCD_ZIP_File> --to-eilcd --skip-external-docs
```

---

## 5. Log File Information

Both data conversion and validation tools will automatically generate execution logs. The log file name is:

```
tidas-{function_name}.log
```

---

## 6. Development Environment Setup and Contribution Guide

If you wish to participate in development, you can set up your environment following these steps:

### (1) Ubuntu System Environment Preparation

```bash
# Update repositories and install software management tools
sudo apt update
sudo apt install software-properties-common

# Add the official PPA repository for the latest Python version and install Python 3.12
sudo add-apt-repository ppa:deadsnakes/ppa
sudo apt install -y python3.12

# Install necessary dependency packages
sudo apt install libxml2-dev libxslt-dev
sudo apt-get install build-essential python3-dev

# Upgrade software packages on the system
sudo apt upgrade
```

### (2) Manage Python Environment with uv

```bash
# Install uv (if not already available)
curl -LsSf https://astral.sh/uv/install.sh | sh

# Synchronize dependencies (including development tools)
uv sync --dev

# Activate the virtual environment created by uv (optional)
source .venv/bin/activate

# Run project commands without activating the environment
uv run python src/tidas_tools/convert.py --help
```

---

## 7. Code Standards and Testing

### (1) Code Formatting Tool (black recommended)

```bash
# Automatically format code using black
uv run black .
```

### (2) Testing Instructions

To test data conversion and validation functionalities, run the following commands:

```bash
# Test converting TIDAS data to eILCD format
uv run python src/tidas_tools/convert.py -i <TIDAS_data_directory> -o <eILCD_data_directory> --to-eilcd

# Test converting eILCD data to TIDAS format
uv run python src/tidas_tools/convert.py --input-dir <eILCD_data_directory> --output-dir <TIDAS_data_directory> --to-tidas

# Test TIDAS data validation functionality
# Execute automated tests
uv run pytest

# Validate TIDAS data
uv run python src/tidas_tools/validate.py -i <eILCD_data_directory>
```

---

## 8. Automatic Building and Publishing (CI/CD)

This project supports automatic building and publishing. When you push a git tag named with the `v<version>` format to the repository, it will trigger the workflow automatically. For example:

```bash
# List existing tags
git tag

# Create a new tag (e.g., version v0.0.1)
git tag v0.0.1

# Push the newly created tag to the remote repository to trigger automatic workflow
git push origin v0.0.1
```

---

## 9. Contribution

We welcome your contributions! You can participate in the project by submitting issues or pull requests.
