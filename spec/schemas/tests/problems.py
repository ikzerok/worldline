"""验证问题协议 Schema；可传 CLI/RPC JSON 文件进一步校验真实输出。"""
import json
from pathlib import Path
import sys
import jsonschema
from referencing import Registry, Resource

BASE = Path(__file__).resolve().parents[1]
result_schema = json.loads((BASE / 'problems.schema.json').read_text())
request_schema = json.loads((BASE / 'problems-request.schema.json').read_text())
registry = Registry().with_resource(result_schema['$id'], Resource.from_contents(result_schema))
for schema in [result_schema, request_schema]:
    jsonschema.Draft202012Validator.check_schema(schema)
result = jsonschema.Draft202012Validator(result_schema, registry=registry)
request = jsonschema.Draft202012Validator(request_schema, registry=registry)
for value in [
    {'path': '.'}, {'project_id': 'p1', 'limit': 0, 'query': {}},
    {'path': '.', 'related_id': 'p1', 'query': {'text': '', 'domains': [], 'path': None}},
    {'path': '.', 'options': {'max_entries': 0}},
]:
    request.validate(value)
for value in [
    {}, {'path': '.', 'project_id': 'p1'}, {'path': '.', 'extra': 1},
    {'path': '.', 'limit': 201}, {'path': '.', 'query': {'unknown': 1}},
    {'path': '.', 'query': {'severities': ['fatal']}},
    {'path': '.', 'options': {'max_entries': 20001}},
    {'path': '.', 'related_id': 'p1', 'query': {'text': 'filtered'}},
    {'path': '.', 'cursor': {'report_version': 'r', 'query_key': 'q', 'offset': 0, 'extra': 1}},
]:
    assert not request.is_valid(value), value
result.validate({'ok': False, 'error': {'code': 'INVALID_QUERY', 'message': '路径无效'}})
for path in sys.argv[1:]:
    for line in Path(path).read_text().splitlines():
        value = json.loads(line)
        result.validate(value.get('result', value))
print('Problems schemas: shape, strict fields and combinations passed')
