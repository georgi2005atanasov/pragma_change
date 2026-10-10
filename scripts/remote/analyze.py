import json
import sys
from nginx_config import parse, locations, fingerprint
source = sys.stdin.read(262145)
try:
    if len(source.encode()) > 262144: raise ValueError('configuration exceeds 256 KiB')
    files = {'uploaded.conf': source}
    parse(source)
    print(json.dumps({'locations': locations(files), 'fingerprint': fingerprint(files), 'notice': 'Upload analysis only. Included files and all runtime references must be discovered and validated on the target server before deployment.'}))
except ValueError as error:
    print(json.dumps({'error':str(error)}))
