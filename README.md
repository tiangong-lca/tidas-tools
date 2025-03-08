
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

poetry lock
poetry install

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


### Lint and Format

```bash
black .
```

### Run Test

```bash
python src/tidas_tools/convert.py -i test_data/converted_json/data/ -o test_data/converted_xml --to-eilcd
python src/tidas_tools/convert.py -i test_data/published_xml/ -o test_data/converted_json/ --to-tidas

python src/tidas_tools/validate.py -i test_data/converted_json/data/
```
