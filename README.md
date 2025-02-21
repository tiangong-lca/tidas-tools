
# TianGong TIDAS Toolbox

## Env Preparing

### Using Ubuntu

```bash

sudo apt update
sudo apt install software-properties-common
sudo add-apt-repository ppa:deadsnakes/ppa
sudo apt install -y python3.12

sudo apt install libxml2-dev libxslt-dev
sudo apt-get install build-essential python3-dev


sudo apt upgrade
```

### Using Poetry

```bash
curl -sSL https://install.python-poetry.org | python3 -

poetry env activate

poetry env info

poetry install

```

### LCA DB Schema Generation

```bash
npm install -g @xata.io/cli@latest
xata auth login
xata schema dump --file src/tools/lca_data_schema/schema_origin.json
```

### Auto Build

The auto build will be triggered by pushing any tag named like release-v$version. For instance, push a tag named as v0.0.1 will build a docker image of 0.0.1 version.

```bash
#list existing tags
git tag
#creat a new tag
git tag v0.0.1
#push this tag to origin
git push origin v0.0.1
```

### LangSmith

```bash
export LANGCHAIN_TRACING_V2=true
export LANGCHAIN_API_KEY=your_api_key
```

### Production Run

```bash
docker build -t 339712838008.dkr.ecr.us-east-1.amazonaws.com/kaiwu-gpts:0.0.1 .

aws ecr get-login-password --region us-east-1  | docker login --username AWS --password-stdin 339712838008.dkr.ecr.us-east-1.amazonaws.com

docker push 339712838008.dkr.ecr.us-east-1.amazonaws.com/kaiwu-gpts:0.0.1
```

### Local Run

```bash
docker run -p 80:80 339712838008.dkr.ecr.us-east-1.amazonaws.com/kaiwu-gpts:0.0.1
```

### secrets.toml

Copy secrets_dev.toml to secrets.toml and fill in the real secrets.

### Lint and Format

```bash
make lint
make format
```