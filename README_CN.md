# TianGong TIDAS Tools 使用说明

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

### （三）使用示例

```bash
# 验证 TIDAS 数据格式
tidas-validate --input-dir <TIDAS数据目录>
```

---

## 四、日志文件说明

数据转换和验证工具执行过程中，会自动生成运行日志，日志文件名为：

```
tidas-tools.log
```

---

## 五、开发环境搭建与代码贡献指南

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

### （二）使用 Poetry 管理 Python 环境

```bash
# 安装 Poetry
curl -sSL https://install.python-poetry.org | python3 -

# 激活 Poetry 环境
poetry env activate

# 显示当前 Poetry 环境信息
poetry env info

# 安装项目依赖包（首次安装需先生成 lock 文件）
poetry lock
poetry install
```

---

## 六、代码规范与测试

### （一）代码格式化工具（推荐使用 black）

```bash
# 使用 black 自动格式化代码
black .
```

### （二）测试工具使用说明

测试项目中的数据转换和验证功能，可以通过以下命令：

```bash
# 测试将 TIDAS 数据转换为 eILCD 格式
python src/tidas_tools/convert.py -i <TIDAS数据目录> -o <eILCD数据目录> --to-eilcd

# 测试将 eILCD 数据转换为 TIDAS 格式
python src/tidas_tools/convert.py --input-dir <eILCD数据目录> --output-dir <TIDAS数据目录> --to-tidas

# 测试 TIDAS 数据验证功能
python src/tidas_tools/validate.py -i <TIDAS数据目录> 
```

---

## 七、自动构建构建并发布（CI/CD）

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

## 八、参与贡献

我们欢迎您的贡献，您可以通过提交 issue 或 pull request 参与到项目中来。
