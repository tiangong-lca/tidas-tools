import xmltodict


def convert_xml_to_json(xml_data):
    return xmltodict.parse(xml_data)


def convert_json_to_xml(json_data):
    return xmltodict.unparse(json_data)
