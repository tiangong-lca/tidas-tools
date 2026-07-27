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
  - Cargo.toml
  - crates/**
  - contracts/**
  - assets/**
  - packaging/**
  - migration/**
  - pyproject.toml
  - src/tidas_tools/**
  - .github/workflows/**
  - scripts/install.*
  - scripts/publish-crates.sh
  - scripts/test-release-request.sh
  - scripts/validate-release-request.sh
  - scripts/sync-rust-package-assets.sh
lastReviewedAt: 2026-07-26
lastReviewedCommit: eed5ed2
lastReviewedNote: "Issue #138 将经过评审、只追加的 Release Request PR 设为精确 native tag 与 release dispatch 的授权入口。"
related:
  - AGENTS.md
  - .docpact/config.yaml
  - docs/agents/repo-validation.md
  - docs/agents/repo-architecture.md
  - README.md
---

# TianGong TIDAS Tools 使用说明

[![crates.io](https://img.shields.io/crates/v/tidas.svg)][crates.io]
[![GitHub Release](https://img.shields.io/github/v/release/tiangong-lca/tidas-tools)][releases]

[crates.io]: https://crates.io/crates/tidas
[releases]: https://github.com/tiangong-lca/tidas-tools/releases

[English](https://github.com/tiangong-lca/tidas-tools/blob/main/README.md) | [中文](https://github.com/tiangong-lca/tidas-tools/blob/main/README_CN.md)

本仓库正在把转换、导入、导出、校验、发布和 ruleset 能力迁移到唯一的跨平台
Rust 可执行文件 `tidas`。

## Rust 迁移预览

当前 Rust 实现已经建立 Cargo workspace、稳定机器与 invocation 契约、有界运行时
基础设施、可执行资产完整性锁、XML/XSD/XSLT 跨平台边界、最终统一 CLI 适配层，
以及原生 TIDAS/ILCD 校验、引用提取、batch 证据、ruleset 检查、双向
TIDAS/eILCD 转换、外部格式导入、数据库导出、确定性 release control 与可复现
原生分发：

```bash
cargo build --workspace
cargo run -p tidas --bin tidas -- --help
cargo run -p tidas --bin tidas -- --format json version
cargo run -p tidas --bin tidas -- convert <TIDAS数据包目录> \
  --output <eILCD数据包目录> --to ilcd --format json
cargo run -p tidas --bin tidas -- convert <eILCD数据目录> \
  --output <TIDAS数据包目录> --to tidas --format json
cargo run -p tidas --bin tidas -- import <源文件或目录> \
  --output <导入输出目录> --target both --write-mapping --format json
cargo run -p tidas --bin tidas -- export \
  --output <数据包.zip> --skip-external-docs --format json
cargo run -p tidas --bin tidas -- validate <数据包目录> \
  --issues <issues.jsonl> --format json
cargo run -p tidas --bin tidas -- validate <ILCD目录> \
  --input-format ilcd-xml --issues <issues.jsonl> --format json
cargo run -p tidas --bin tidas -- release build-packages \
  --tidas-dir <canonical-TIDAS目录> \
  --dataset-index <canonical-dataset-index.json> \
  --output-dir <release目录> --format json
cargo run -p tidas --bin tidas -- ruleset --format json
cargo run -p tidas --bin tidas -- --completion bash > tidas.bash
cargo run -p tidas-assets --bin tidas-asset-lock -- check
cargo run -p tidas-dist -- version
```

最终命令树固定为 `convert`、`import`、`export`、`validate`、`release`、
`ruleset` 和 `version`。七个命令均已由 Rust 实现，且都不会调用 Python。

原生导入支持 EcoSpold 1/2、SimaPro CSV、openLCA JSON-LD、openLCA process
XLSX 与 ILCD 文件、目录或 ZIP 包。默认通过有界签名检测源格式，也可使用
`--from-format` 处理歧义输入。命令始终在内部写出并校验 TIDAS，可通过
`--target ilcd|both` 发布 ILCD，默认写出每个 process 的依赖 bundle，并通过
`--write-mapping` 启用确定性的 `mapping.csv.gz`。`.zolca` 会被明确拒绝。
解析、exchange 与 issue 报告均使用可取消、有内存预算且落盘的有界流；失败时不会
发布部分输出。

原生转换把输入镜像到 `OUTPUT/data`，保留数据包元数据，写入经过完整性锁验证的
目标 schemas/stylesheets/methodologies，并原子发布整个输出目录。含顶层扩展元数据
的 TIDAS 文档使用确定性的 `.tidas-envelope.json` sidecar，使 eILCD XML 保持单根，
反向转换时再无损恢复原包络。遍历拒绝符号链接和 XML 1.0 非法字符；相同输入的成功
运行会返回相同的输出树 SHA-256。

原生导出在单个 repeatable-read、只读 PostgreSQL 快照中读取 active records，
通过有界队列流式处理，归一化 TIDAS 数据包版本，可选流式下载 S3-compatible
附加文档，并原子发布一个确定性 ZIP。数据库使用 `TIDAS_DATABASE_URL`；对象存储
凭据只接受 `TIDAS_S3_ACCESS_KEY_ID`、`TIDAS_S3_SECRET_ACCESS_KEY` 与可选的
`TIDAS_S3_SESSION_TOKEN`，其值不会进入报告或诊断。

原生校验只解析内嵌且经过完整性锁验证的 schemas。通过 `--issues` 可把全部问题
按确定顺序原子写入 JSONL；operation report 只保留有界计数与 spool hash，不在
内存中累计问题数组。ILCD XML 使用离线复用的 XSD context 和相同有界报告契约；
`document-validation-batch.v1` 提供 manifest 预检、漂移防护 issue 事件和确定性
final evidence hash。
校验进度按有界频率只写入 stderr；非交互运行可使用 `--progress always`。

全局运行参数遵循“命令行 > `TIDAS_*` 环境变量 > 内置默认值”的优先级，不会隐式
读取当前目录中的配置文件。stdout 只包含一次 human/JSON 报告或 completion 脚本；
日志、进度、诊断与报告落盘确认写入 stderr。使用 `--report <PATH>` 可在不占用
stdout 的情况下持久化报告。默认计入内存预算为 512 MiB，有界队列容量为 256。
规范契约见 [docs/agents/cli-contract.md](docs/agents/cli-contract.md)。

## 原生分发

预构建的 GitHub 归档是面向最终用户的主要渠道；其中已包含原生 XML 依赖，运行时
不需要 Rust 或开发工具链。
原生 release workflow 从同一个精确的 `tidas` binary 构建并验证 Linux
x86_64/ARM64、macOS Intel/Apple Silicon 与 Windows x86_64 制品。每个平台均
重复构建归档并逐字节比较，验证 SHA-256，执行打包后的 `version`、help、JSON
`version` 与 `ruleset` 探针，并生成 SPDX SBOM 和 GitHub OIDC
provenance/SBOM attestation。固定版本且静态链接的 libxml2/libxslt 使运行时
不依赖 Homebrew、vcpkg、Python、Java、Node.js 或开发工具链。

同一个 `v<version>` release 会先把 `tidas` 源码包与全部可复用领域 crates
发布到 crates.io，再创建不可变的 GitHub Release。已经安装 Rust 1.88+ 以及平台
libxml2/libxslt 开发依赖的开发者，也可从源码安装唯一的统一 executable：

```bash
cargo install tidas --version 0.1.0 --locked
```

全部公开 workspace crates 使用完全相同的精确版本，避免 Cargo 混用不兼容的领域
release；`tidas-dist` 始终是仓库内部 release tooling，不会发布。Pull request 会在
无凭据条件下执行完整 multi-package `cargo package` 验证与 crates.io dry-run；
只有 tag context 下的 release workflow 能读取 `CARGO_REGISTRY_TOKEN`。

原生版本通过评审并合并一个只追加的
`.github/releases/v<version>.json` 来授权；该请求把版本绑定到完整 commit SHA。
Pull request 阶段只有只读、无 secret 的校验。合并 job 创建或验证精确 lightweight
tag，再显式从该 tag dispatch 原生 release workflow，使 artifact provenance
绑定到实际发布的源码 commit，而不是请求 PR 的 merge commit。

原生版本发布后，可安装明确且不可变的版本：

```bash
curl --proto '=https' --tlsv1.2 -fsSLO \
  https://raw.githubusercontent.com/tiangong-lca/tidas-tools/main/scripts/install.sh
sh install.sh --version 0.1.0 --prefix "$HOME/.local"
```

```powershell
.\scripts\install.ps1 -Version 0.1.0
```

每个 GitHub Release 同时携带由相同归档哈希生成的 Homebrew formula 与 Winget
manifests。创建外部 tap 或提交 Winget community 需要单独批准，且这些路径绝不
重新构建 executable。Windows ARM64 是明确跟踪的第二阶段目标。

下文记录的 Python 包已经 feature freeze，只在迁移期间作为内部 golden/parity
oracle。它不是最终产品，旧可执行文件名和参数布局不会保留。只有 Rust 功能语义、
确定性输出、性能/RSS、首批跨平台制品、下游切换与 workspace 清理全部通过后，
才会删除 Python。进度见
[Issue #117](https://github.com/tiangong-lca/tidas-tools/issues/117)。

---

## 冻结的 Python oracle 参考

下文说明迁移期间用于 parity 验证的 Python oracle。

---

## 一、Oracle 范围

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

Flow 导入严格保留来源证据。Elementary Flow 名称只写入 `baseName`，不再生成
qualifier；Product、Waste 和 Other Flow 必须同时具备来源支持的 `baseName`、
`treatmentStandardsRoutes` 与 `mixAndLocationTypes`，缺失时会在发布任何 package
之前由 preflight 阻断。Elementary 分类使用不可变的官方 ILCD 树与版本化
`tidas-ef-extension` overlay；无法匹配的来源路径会回退到 air-unspecified，并写入明确
warning。包括 Mass 与净热值在内的全部来源 flow-property assignment 都保留 UUID、
reference 决策、来源顺序与十进制 factor。

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

`tidas release` 消费已经完成 UUID/version 决策的 canonical TIDAS 数据树和 `tiangong.release.canonical-dataset-index.v1`，自身不分配 UUID 或版本。可复用的 Rust release domain 负责精确引用闭包、schema-order ILCD 派生与验证、归一化语义 round-trip，以及固定 ZIP 成员顺序、时间和权限的确定性打包。

```bash
tidas release validate-tidas --input-dir <canonical-tidas目录>
tidas release convert-ilcd --input-dir <canonical-tidas目录> --output-dir <ilcd目录>
tidas release validate-ilcd --input-dir <ilcd目录>
tidas release semantic-roundtrip --tidas-dir <canonical-tidas目录> --ilcd-dir <ilcd目录>
tidas release validate-closure \
  --input-dir <canonical-tidas目录> \
  --dataset-index <canonical-dataset-index.json> \
  --profile unit-process-full-closure.v1
tidas release build-packages \
  --tidas-dir <canonical-tidas目录> \
  --dataset-index <canonical-dataset-index.json> \
  --output-dir <发布包目录> \
  --format json
```

打包命令先完成全部原生校验、转换、闭包包含关系和 round-trip 门禁，再原子发布恰好四个 archive：分别为 `unit-process-full-closure.v1` 与 `standalone-lifecyclemodel-result-full-closure.v1` 的 canonical TIDAS 和派生 ILCD 变体。缺少精确 UUID/version 引用时会 fail closed；ZIP 成员按固定顺序、时间和权限写入。stdout JSON 符合 `tidas.release-report.v1`；可用 `--report <路径>` 原子保存同一 operation report。

---

## 五、TIDAS 与 eILCD/ILCD 数据验证工具使用说明

### （一）工具功能说明

本工具用于验证 TIDAS JSON 数据或 eILCD/ILCD XML 数据是否符合随包提供的 schema 规范要求。TIDAS JSON 校验会先使用编译型 schema 快速路径，发现 schema 问题时再回退到完整错误收集。

### （二）统一 CLI 参数说明

| 参数 | 缩写 | 参数说明 |
|------|------|----------|
| `--help` | `-h` | 显示帮助信息 |
| `<INPUT>` | | 待验证的 package 或 batch 文档目录 |
| `--input-format` | | 输入格式：`tidas-json`（默认）或 `ilcd-xml` |
| `--issues` | | 将确定性的 package issue 事件保存为 JSONL |
| `--describe --format json` | | 输出支持的校验协议以及 package/engine/Schema-lock 指纹 |
| `--protocol document-validation-batch.v1` | | 只校验 JSONL manifest 明确列出的文档，并流式输出 issue/final 事件 |
| `--input-manifest` | | Batch JSONL manifest，包含 opaque document key、安全相对路径、精确身份和 SHA-256 |
| `--events` | | 将确定性的 batch issue/final 事件保存为 JSONL |

### （三）使用示例

```bash
# 验证 TIDAS 数据格式
tidas validate <TIDAS数据目录> --input-format tidas-json --format json

# 验证 eILCD/ILCD XML 数据格式
tidas validate <eILCD数据目录> --input-format ilcd-xml --format json

# 查看闭包预检 Worker 使用的可复现握手信息
tidas validate --describe --format json

# 对 manifest 中明确列出的文档流式生成确定性校验证据
tidas validate <batch根目录> \
  --protocol document-validation-batch.v1 \
  --input-manifest <document-validation-batch.v1.jsonl> \
  --events <validation-events.jsonl> \
  --format json

# 查看或选择 integrity-locked 的原生 ruleset catalog
tidas ruleset --format json
tidas ruleset --id process-authoring/strict --format json
```

Batch 协议把数据问题视为一次正常完成的扫描：逐条输出 `issue`，最后输出摘要和逻辑 hash，并以 0 退出。路径越界、重复 key/path、符号链接、内容 hash 漂移、manifest 非法或执行完成证据缺失属于协议/系统故障。引用目标是否存在及数据库可见性不属于文档校验层。

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

## 十、Release 授权与 SDK Dispatch

维护者通过经过评审的 Release Request PR 发布原生版本。只追加的请求文件声明版本
并固定完整源码 commit；合并该 PR 就是不可逆的发布授权。自动化随后创建匹配 tag，
并从该 tag dispatch 完整 crates.io 与五平台 GitHub Release workflow。手工向
canonical 仓库推 tag 只保留为恢复路径，不是普通贡献者的发布方式。

当 `main` 上的 schema 或 methodology 路径变化时，`.github/workflows/dispatch-tidas-sdk-sync.yml` 也可以触发 `tiangong-lca/tidas-sdk` 的下游 SDK 同步。

该自动化需要仓库 secret：`TIDAS_SDK_AUTOMATION_TOKEN`。

---

## 十一、参与贡献

我们欢迎您的贡献，您可以通过提交 issue 或 pull request 参与到项目中来。
