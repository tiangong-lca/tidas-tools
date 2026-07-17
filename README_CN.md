---
title: tidas-tools README CN
docType: guide
scope: repo
status: active
authoritative: false
owner: tidas-tools
language: zh-CN
whenToUse:
  - when you need Chinese user-facing CLI examples or basic development commands
whenToUpdate:
  - when Chinese CLI examples, development commands, or release notes change
checkPaths:
  - README_CN.md
  - AGENTS.md
  - .docpact/config.yaml
  - docs/agents/**
  - pyproject.toml
  - src/tidas_tools/**
  - .github/workflows/**
lastReviewedAt: 2026-04-24
lastReviewedCommit: 7984b9bc9f820da7bc31520e8334c9fddedc85d4
related:
  - AGENTS.md
  - .docpact/config.yaml
  - docs/agents/repo-validation.md
  - docs/agents/repo-architecture.md
  - README.md
---

# TianGong TIDAS Tools 使用说明

[![PyPI](https://img.shields.io/pypi/v/tidas-tools.svg)][pypi status]
[![Python Version](https://img.shields.io/pypi/pyversions/tidas-tools)][pypi status]

[pypi status]: https://pypi.org/project/tidas-tools/

[English](https://github.com/tiangong-lca/tidas-tools/blob/main/README.md) | [中文](https://github.com/tiangong-lca/tidas-tools/blob/main/README_CN.md)

本工具箱用于 TianGong TIDAS 与 eILCD/ILCD 数据格式的转换和验证。

---

## 一、工具简介

本工具箱包含以下独立工具：

- **TIDAS 与 eILCD 数据格式转换工具**
- **外部 LCA 数据格式导入工具**
- **TIDAS 与 eILCD/ILCD 数据验证工具**
- **TIDAS 与 eILCD 数据导出工具**

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

## 三、外部 LCA 数据格式导入工具使用说明

### （一）当前范围

`tidas-import` 是将外部 LCA 数据格式导入为 TIDAS，并可后续输出 ILCD/eILCD 的分阶段入口。当前实现提供命令行入口、源格式检测、`.zolca` 明确拒绝、机器可读 conversion report，以及 openLCA JSON-LD、EcoSpold 1、SimaPro CSV、EcoSpold 2 和 openLCA process XLSX 的最小可验证导入适配器。

当前源格式状态：

- openLCA JSON-LD zip/目录：最小导入到 TIDAS 和 ILCD/eILCD
- EcoSpold 1 XML/zip：最小导入到 TIDAS 和 ILCD/eILCD
- SimaPro CSV block format：最小导入到 TIDAS 和 ILCD/eILCD
- EcoSpold 2 `.spold`/zip：最小导入到 TIDAS 和 ILCD/eILCD
- openLCA process XLSX：最小导入到 TIDAS 和 ILCD/eILCD

`.zolca` 按本轮范围要求明确排除。

导入的 JSON-LD Actor 和 Source 会写出为 TIDAS contact 与 source。EcoSpold、SimaPro CSV 和 process XLSX 源数据中的单位会在缺少显式 reference data 时生成对应 unit group 与 flow property，减少全部 flow 落到默认 `Mass`/`kg` 的情况。

当下游 AI/导入 worker 需要按 process 并行处理时，导入器默认写出
process bundle。标准 `<输出目录>/tidas` 包会保持原样写出；导入器会额外写出
`<输出目录>/process-bundles/<process_uuid>/` 子目录，其中包含该 process JSON 以及它引用的
flow、flow property、unit group、contact 和 source JSON 文件。可用
`--process-bundles-dir <目录>` 覆盖默认 bundle 位置，也可用
`--no-process-bundles` 关闭 bundle 输出。

专家审查用 mapping CSV 默认关闭，因为大型导入会生成很大的逐字段映射文件。
需要时可用 `--write-mapping-csv` 写出
`<输出目录>/mapping.csv.gz`。

### （二）使用示例

```bash
tidas-import --input <源文件或目录> --output-dir <输出目录> --detect-only
tidas-import --input <源文件或目录> --output-dir <输出目录> --target both --validation-jobs 0
tidas-import --input <源文件或目录> --output-dir <输出目录> --no-process-bundles
tidas-import --input <源文件或目录> --output-dir <输出目录> --write-mapping-csv
```

---

## 四、确定性 TIDAS/ILCD Release 打包

`tidas-release-tool` 消费已经完成 UUID/version 决策的 canonical TIDAS 数据树和 `tiangong.release.canonical-dataset-index.v1`，自身不分配 UUID 或版本。它负责精确引用闭包、ILCD 转换与验证、归一化语义 round-trip，以及固定 ZIP 成员顺序、时间和权限的确定性打包。

```bash
tidas-release-tool validate-tidas --input-dir <canonical-tidas目录>
tidas-release-tool convert-ilcd --input-dir <canonical-tidas目录> --output-dir <ilcd目录>
tidas-release-tool validate-ilcd --input-dir <ilcd目录>
tidas-release-tool semantic-roundtrip --tidas-dir <canonical-tidas目录> --ilcd-dir <ilcd目录>
tidas-release-tool build-packages \
  --tidas-dir <canonical-tidas目录> \
  --ilcd-dir <ilcd目录> \
  --dataset-index <canonical-dataset-index.json> \
  --output-dir <发布包目录>
```

打包命令分别为 `unit-process-full-closure.v1` 与 `standalone-lifecyclemodel-result-full-closure.v1` 生成 canonical TIDAS 和派生 ILCD 变体。缺少精确 UUID/version 引用时会 fail closed。stdout 为稳定 JSON；可用 `--report <路径>` 同时保存报告。

---

## 五、TIDAS 与 eILCD/ILCD 数据验证工具使用说明

### （一）工具功能说明

本工具用于验证 TIDAS JSON 数据或 eILCD/ILCD XML 数据是否符合随包提供的 schema 规范要求。TIDAS JSON 校验会先使用编译型 schema 快速路径，发现 schema 问题时再回退到完整错误收集。

### （二）命令行参数说明

| 参数 | 缩写 | 参数说明 |
|------|------|----------|
| `--help` | `-h` | 显示帮助信息 |
| `--input-dir` | `-i` | 待验证数据所在目录 |
| `--verbose` | `-v` | 开启详细日志模式 |
| `--data-format` | | 待验证的数据格式：`tidas`、`ilcd` 或 `eilcd`（默认：`tidas`） |
| `--jobs` | | 并行校验进程数；使用 `0` 表示使用全部 CPU 核心 |

### （三）使用示例

```bash
# 验证 TIDAS 数据格式
tidas-validate --input-dir <TIDAS数据目录> --data-format tidas

# 验证 eILCD/ILCD XML 数据格式
tidas-validate --input-dir <eILCD数据目录> --data-format ilcd

# 使用全部 CPU 核心校验大型数据包
tidas-validate --input-dir <TIDAS数据目录> --data-format tidas --jobs 0
```

## 六、TIDAS 数据导出工具使用说明

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

## 七、日志文件说明

数据转换和验证工具执行过程中，会自动生成运行日志，日志文件名为：

```
tidas-{function_name}.log
```

---

## 八、开发环境搭建与代码贡献指南

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

## 九、代码规范与测试

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

# 测试外部 LCA 格式检测
uv run python src/tidas_tools/import_lca/cli.py --input <源文件或目录> --output-dir <输出目录> --detect-only

# 测试 TIDAS 与 eILCD/ILCD 数据验证功能
# 执行自动化测试
uv run pytest

# 验证 TIDAS 数据
uv run python src/tidas_tools/validate.py -i <TIDAS数据目录> --data-format tidas

# 验证 eILCD/ILCD 数据
uv run python src/tidas_tools/validate.py -i <eILCD数据目录> --data-format ilcd
```

---

## 十、自动构建构建并发布（CI/CD）

本项目支持自动构建和发布，当您向 git 仓库推送以 `v版本号` 命名的 tag 时，会自动触发。例如：

```bash
# 列出已有的 tag
git tag

# 创建新 tag（例如版本 v0.0.1）
git tag v0.0.1

# 将新创建的 tag 推送到远程仓库，触发自动构建和发布
git push origin v0.0.1
```

当 `main` 上的 schema 或 methodology 路径变化时，`.github/workflows/dispatch-tidas-sdk-sync.yml` 也可以触发 `tiangong-lca/tidas-sdk` 的下游 SDK 同步。

该自动化需要仓库 secret：`TIDAS_SDK_AUTOMATION_TOKEN`。

---

## 十一、参与贡献

我们欢迎您的贡献，您可以通过提交 issue 或 pull request 参与到项目中来。
