# TianGong TIDAS Tools 使用说明

[![PyPI](https://img.shields.io/pypi/v/tidas-tools.svg)][pypi status]
[![Python Version](https://img.shields.io/pypi/pyversions/tidas-tools)][pypi status]

[pypi status]: https://pypi.org/project/tidas-tools/

[English](https://github.com/tiangong-lca/tidas-tools/blob/main/README.md) | [中文](https://github.com/tiangong-lca/tidas-tools/blob/main/README_CN.md)

本工具箱用于 TianGong TIDAS 数据格式的转换和验证。

---

## 一、工具简介

本工具箱包含两个独立工具：

- **TIDAS 与 eILCD 数据格式转换工具**
- **TIDAS 数据验证工具**

---

## 二、TIDAS 与 eILCD 数据格式转换工具使用说明

### （一）安装说明

```bash
# 安装本工具箱
pip install tidas-tools
```

### （二）工具功能说明

本工具用于以下两种数据格式的互相转换：

- TIDAS 数据格式 转换为 eILCD 数据格式（默认模式）
- eILCD 数据格式 转换为 TIDAS 数据格式

### （三）命令行参数说明

| 参数 | 缩写 | 参数说明 |
|------|------|----------|
| `--help` | `-h` | 显示帮助信息 |
| `--input-dir` | `-i` | 待转换数据所在的目录（注意：该目录应直接包含数据文件，而非其上层目录） |
| `--output-dir` | `-o` | 转换后数据输出目录（程序会自动生成包含完整 schema 的目录结构） |
| `--to-eilcd` | | 将数据从 TIDAS 格式转换为 eILCD 格式（默认模式） |
| `--to-tidas` | | 将数据从 eILCD 格式转换为 TIDAS 格式 |
| `--verbose` | `-v` | 开启详细日志模式 |

### （四）使用示例

```bash
# 将 TIDAS 数据转换为 eILCD 数据格式
tidas-convert --input-dir <TIDAS数据目录> --output-dir <eILCD数据输出目录> --to-eilcd

# 将 eILCD 数据转换为 TIDAS 数据格式
tidas-convert --input-dir <eILCD数据目录> --output-dir <TIDAS数据输出目录> --to-tidas
```

---

## 三、TIDAS 数据验证工具使用说明

### （一）工具功能说明

本工具用于验证 TIDAS 数据格式是否符合规范要求。

### （二）命令行参数说明

| 参数 | 缩写 | 参数说明 |
|------|------|----------|
| `--help` | `-h` | 显示帮助信息 |
| `--input-dir` | `-i` | 待验证的 TIDAS 数据所在目录（注意：该目录应直接包含数据文件，而非其上层目录）|
| `--verbose` | `-v` | 开启详细日志模式 |

### （三）使用示例

```bash
# 验证 TIDAS 数据格式
tidas-validate --input-dir <TIDAS数据目录>
```

## 四、TIDAS 数据导出工具使用说明

### （一）工具功能说明

本工具用于从数据库导出记录为指定格式（TIDAS 或 eILCD），并可选择是否下载附加文件，最终输出为zip压缩文件。

### （二）命令行与环境变量参数

| 参数 | 缩写 | 参数说明 |
| --- | --- | --- |
| `--help` | `-h` | 显示帮助信息 |
| `--input-dir` | `-i` | 存储导出文件的输入目录（TIDAS或eILCD格式） |
| `--output-zip` | `-z` | 输出的zip文件名（无需包含.zip扩展名） |
| `--env-file` | `-e` | 包含数据库和AWS凭证的.env文件路径 |
| `--to-tidas` | 无 | 输出为TIDAS格式（默认选项） |
| `--to-eilcd` | 无 | 输出为EILCD格式（与`--to-tidas`互斥） |
| `--db-user` | 无 | 数据库用户名 |
| `--db-password` | 无 | 数据库密码 |
| `--db-host` | 无 | 数据库主机地址 |
| `--db-port` | 无 | 数据库端口（默认5432） |
| `--db-name` | 无 | 数据库名称 |
| `--aws-access-key-id` | 无 | AWS访问密钥ID |
| `--aws-secret-access-key` | 无 | AWS秘密访问密钥 |
| `--aws-region` | 无 | AWS区域 |
| `--aws-endpoint` | 无 | AWS端点URL |
| `--aws-bucket` | 无 | AWS S3存储桶名称（用于附加文件） |
| `--skip-external-docs` | 无 | 跳过附加文件下载 |
| `--verbose` | `-v` | 启用详细日志模式 |

您也可以使用环境变量来设置数据库和AWS凭证（默认当前路径下的.env文件）：

```env
DB_USER=
DB_PASSWORD=
DB_HOST=
DB_PORT=5432
DB_NAME=postgres
AWS_REGION=
AWS_ENDPOINT=
AWS_EXTERNAL_DOCS_BUCKET=external_docs
AWS_ACCESS_KEY_ID=
AWS_SECRET_ACCESS_KEY=
```

### （三）使用示例

```bash
# 导出记录为TIDAS格式并创建压缩文件
tidas-export --tidas-dir <TIDAS数据目录> --output-zip <TIDAS ZIP文件> --to-tidas

# 导出记录为eILCD格式，并跳过附加文件下载
tidas-export -z <eILCD ZIP文件> --to-eilcd --skip-external-docs
```
---

## 五、日志文件说明

数据转换和验证工具执行过程中，会自动生成运行日志，日志文件名为：

```
tidas-{function_name}.log
```

---

## 六、开发环境搭建与代码贡献指南

如果您希望参与开发贡献，您可以参考以下步骤搭建开发环境：

### （一）Ubuntu 系统环境准备

```bash
# 更新软件源并安装软件管理工具
sudo apt update
sudo apt install software-properties-common

# 添加 Python 最新版本的官方 PPA 源，并安装 Python 3.12
sudo add-apt-repository ppa:deadsnakes/ppa
sudo apt install -y python3.12

# 安装必要的依赖包
sudo apt install libxml2-dev libxslt-dev
sudo apt-get install build-essential python3-dev

# 升级系统上的软件
sudo apt upgrade
```

### （二）使用 uv 管理 Python 环境

```bash
# 安装 uv（如已安装可跳过）
curl -LsSf https://astral.sh/uv/install.sh | sh

# 同步项目依赖（包含开发工具）
uv sync --dev

# 激活 uv 创建的虚拟环境（可选）
source .venv/bin/activate

# 在未激活环境的情况下执行命令
uv run python src/tidas_tools/convert.py --help
```

---

## 七、代码规范与测试

### （一）代码格式化工具（推荐使用 black）

```bash
# 使用 black 自动格式化代码
uv run black .
```

### （二）测试工具使用说明

测试项目中的数据转换和验证功能，可以通过以下命令：

```bash
# 测试将 TIDAS 数据转换为 eILCD 格式
uv run python src/tidas_tools/convert.py -i <TIDAS数据目录> -o <eILCD数据目录> --to-eilcd

# 测试将 eILCD 数据转换为 TIDAS 格式
uv run python src/tidas_tools/convert.py --input-dir <eILCD数据目录> --output-dir <TIDAS数据目录> --to-tidas

# 测试 TIDAS 数据验证功能
# 执行自动化测试
uv run pytest

# 验证 TIDAS 数据
uv run python src/tidas_tools/validate.py -i <TIDAS数据目录> 
```

---

## 八、自动构建构建并发布（CI/CD）

本项目支持自动构建和发布，当您向 git 仓库推送以 `v版本号` 命名的 tag 时，会自动触发。例如：

```bash
# 列出已有的 tag
git tag

# 创建新 tag（例如版本 v0.0.1）
git tag v0.0.1

# 将新创建的 tag 推送到远程仓库，触发自动构建和发布
git push origin v0.0.1
```

---

## 九、参与贡献

我们欢迎您的贡献，您可以通过提交 issue 或 pull request 参与到项目中来。
