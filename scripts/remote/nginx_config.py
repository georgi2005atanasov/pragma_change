"""Lossless, deliberately conservative Nginx configuration inspection and patching."""
import dataclasses
import difflib
import hashlib
import json


@dataclasses.dataclass
class Node:
    words: list
    start: int
    opening: int | None
    end: int
    children: list


def parse(source):
    tokens = []
    i = 0
    while i < len(source):
        c = source[i]
        if c.isspace():
            i += 1
            continue
        if c == '#':
            n = source.find('\n', i)
            i = len(source) if n < 0 else n + 1
            continue
        start = i
        if c in '{};':
            tokens.append((c, i, i + 1)); i += 1; continue
        word, quote = '', None
        while i < len(source):
            c = source[i]
            if c == '\\':
                if i + 1 == len(source): raise ValueError('unfinished escape')
                word += source[i:i + 2]; i += 2; continue
            if quote:
                if c == quote: quote = None
                else: word += c
                i += 1; continue
            if c in "\"'": quote = c; i += 1; continue
            if source.startswith('${', i):
                end = source.find('}', i + 2)
                if end < 0: raise ValueError('unfinished variable')
                word += source[i:end + 1]; i = end + 1; continue
            if c.isspace() or c in '{};#': break
            word += c; i += 1
        if quote: raise ValueError('unfinished quote')
        if not word: raise ValueError('empty directive token')
        tokens.append((word, start, i))
    position = 0
    def block(nested=False):
        nonlocal position
        result = []
        while position < len(tokens):
            if tokens[position][0] == '}':
                if not nested: raise ValueError('unexpected closing brace')
                closing = tokens[position][2]; position += 1
                return result, closing
            start, words = tokens[position][1], []
            while position < len(tokens) and tokens[position][0] not in '{};':
                words.append(tokens[position][0]); position += 1
            if not words or position == len(tokens): raise ValueError('incomplete directive')
            token, opening, end = tokens[position]; position += 1
            if token == '}': raise ValueError('missing semicolon')
            children = []
            if token == '{': children, end = block(True)
            result.append(Node(words, start, opening if token == '{' else None, end, children))
        if nested: raise ValueError('unclosed block')
        return result, len(source)
    return block()[0]


def fingerprint(files):
    return hashlib.sha256(json.dumps(files, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def locations(files):
    result = []
    def walk(path, nodes, parents):
        for node in nodes:
            if node.words[0] == 'location':
                key = hashlib.sha256(f'{path}:{node.start}'.encode()).hexdigest()[:24]
                result.append({'id': key, 'file': path, 'location': ' '.join(node.words[1:]),
                               'context': parents, 'offset': node.opening})
            walk(path, node.children, parents + [' '.join(node.words)])
    for path, source in files.items(): walk(path, parse(source), [])
    return result


def prepare(files, main, selected, module, root, socket):
    """Return exact candidate files; refuse already managed/unrecognized structures."""
    if len(selected) != len(set(selected)) or not selected: raise ValueError('select distinct locations')
    candidates = {x['id']: x for x in locations(files)}
    if any(x not in candidates for x in selected): raise ValueError('location selection is stale')
    if any('pragmachange' in source for source in files.values()):
        raise ValueError('installation already contains Pragma directives; use release deployment or review the existing integration manually')
    nodes = parse(files[main])
    http = [n for n in nodes if n.words == ['http'] and n.opening is not None]
    if len(http) != 1: raise ValueError('a single top-level http block is required for automatic integration')
    # A location in an included file has no syntactic http parent. nginx -t is authoritative.
    for key in selected:
        context = candidates[key]['context']
        if any(x.split()[0] in ('stream', 'mail', 'if', 'location') for x in context):
            raise ValueError('nested, conditional, or non-HTTP locations require manual integration')
    edits = {main: [(0, f'include {root}/managed-module.conf;\n'),
                    (http[0].opening + 1, f'\n    include {root}/managed-http.conf;\n')]}
    for key in selected:
        loc = candidates[key]
        edits.setdefault(loc['file'], []).append((loc['offset'] + 1, '\n        pragmachange on;\n'))
    output = dict(files)
    for path, changes in edits.items():
        for offset, text in sorted(changes, reverse=True): output[path] = output[path][:offset] + text + output[path][offset:]
    output[f'{root}/managed-module.conf'] = f'load_module {module};\n'
    output[f'{root}/managed-http.conf'] = (
        f'pragmachange_zone 64m;\npragmachange_release {root}/current/release.json;\n'
        f'pragmachange_secret_file {root}/partition_secret;\n'
        f'server {{ listen unix:{socket}; location = /status {{ pragmachange_status; }} }}\n')
    diff = '\n'.join(''.join(difflib.unified_diff(files.get(p, '').splitlines(True), text.splitlines(True),
                       fromfile=p, tofile=p, n=0)) for p, text in output.items() if files.get(p) != text)
    return output, diff
