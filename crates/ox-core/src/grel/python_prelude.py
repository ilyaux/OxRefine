# OxRefine's Python expressions: OpenRefine 3.10.1 runs them in Jython 2.7; OxRefine runs them in
# RustPython (Python 3). This prelude makes common Python 2 code behave as it does in Jython:
# integer division, print statements, `except E, e`, backticks, <>, Python 2 builtins, ASCII-only
# regex classes, and round() halves away from zero. tests/langs/known_failures.txt lists what differs.
import ast as _ast
import base64 as _base64
import builtins as _builtins
import datetime as _datetime
from decimal import ROUND_HALF_UP as _ROUND_HALF_UP, Decimal as _Decimal
import functools as _functools
import hashlib as _hashlib
import math as _math
import numbers as _numbers
import re as _re
import string as _string
import urllib as _urllib
import urllib.parse as _urlparse


def _py2_div(a, b):
    if isinstance(a, int) and isinstance(b, int):
        return a // b
    return a / b


def _py2_round(x, n=0):
    # Python 2 rounds the exact binary value half away from zero: round(2.5) == 3.0, round(2.675, 2) == 2.67
    q = _Decimal(1).scaleb(-n)
    return float(_Decimal(x).quantize(q, rounding=_ROUND_HALF_UP))


def _bytes(s):
    return s.encode("utf-8") if isinstance(s, str) else s


def _py2_filter(f, xs):
    out = _builtins.filter(f, xs)
    if isinstance(xs, str):
        return "".join(out)
    return tuple(out) if isinstance(xs, tuple) else list(out)


def _cmp(a, b):
    return (a > b) - (a < b)


_PY2 = {
    "unicode": str,
    "basestring": str,
    "long": int,
    "xrange": range,
    "unichr": chr,
    "reduce": _functools.reduce,
    "cmp": _cmp,
    "round": _py2_round,
    "map": lambda f, *xs: list(_builtins.map(f, *xs)) if f is not None else list(_builtins.zip(*xs)),
    "filter": lambda f, xs: _py2_filter(f, xs),
    "zip": lambda *xs: list(_builtins.zip(*xs)),
    "__ox_div__": _py2_div,
}

# Python 2 module functions that common expressions still use.
for _name in ("upper", "lower", "strip", "lstrip", "rstrip", "split", "replace", "capitalize", "swapcase", "find", "rfind", "count", "zfill", "center", "ljust", "rjust"):
    setattr(_string, _name, getattr(str, _name))
_string.join = lambda words, sep=" ": sep.join(words)
_string.letters = _string.ascii_letters
_string.lowercase = _string.ascii_lowercase
_string.uppercase = _string.ascii_uppercase
for _name in ("md5", "sha1", "sha224", "sha256", "sha384", "sha512"):
    _h = getattr(_hashlib, _name)
    setattr(_hashlib, _name, (lambda h: lambda data=b"", *a, **k: h(_bytes(data), *a, **k))(_h))
for _name in ("b64encode", "b64decode", "b32encode", "b32decode", "b16encode", "b16decode", "urlsafe_b64encode", "urlsafe_b64decode", "encodestring", "decodestring"):
    if hasattr(_base64, _name):
        _f = getattr(_base64, _name)
        setattr(_base64, _name, (lambda f: lambda s, *a, **k: f(_bytes(s), *a, **k))(_f))
for _name in ("quote", "quote_plus", "unquote", "unquote_plus", "urlencode"):
    setattr(_urllib, _name, getattr(_urlparse, _name))

# Python 2 regex classes (\w, \d, \s, \b) are ASCII unless re.UNICODE is given.
_re_compile = _re._compile


def _ascii_compile(pattern, flags):
    if isinstance(pattern, str) and not (flags & _re.UNICODE):
        flags |= _re.ASCII
    return _re_compile(pattern, flags)


_re._compile = _ascii_compile


_ITER = {"iteritems": "items", "iterkeys": "keys", "itervalues": "values"}


class _Py2(_ast.NodeTransformer):
    """Integer division, d.has_key(k) and d.iteritems(), rewritten in the syntax tree."""

    def visit_Call(self, node):
        self.generic_visit(node)
        f = node.func
        if isinstance(f, _ast.Attribute) and f.attr == "has_key" and len(node.args) == 1 and not node.keywords:
            return _ast.copy_location(_ast.Compare(left=node.args[0], ops=[_ast.In()], comparators=[f.value]), node)
        if isinstance(f, _ast.Attribute) and f.attr in _ITER and not node.args:
            f.attr = _ITER[f.attr]
        return node

    def visit_BinOp(self, node):
        self.generic_visit(node)
        if isinstance(node.op, _ast.Div):
            return _ast.copy_location(_ast.Call(func=_ast.Name(id="__ox_div__", ctx=_ast.Load()), args=[node.left, node.right], keywords=[]), node)
        return node

    def visit_AugAssign(self, node):
        self.generic_visit(node)
        if isinstance(node.op, _ast.Div) and isinstance(node.target, _ast.Name):
            value = _ast.Call(func=_ast.Name(id="__ox_div__", ctx=_ast.Load()), args=[_ast.Name(id=node.target.id, ctx=_ast.Load()), node.value], keywords=[])
            return _ast.copy_location(_ast.Assign(targets=[node.target], value=value), node)
        return node


_STRING = _re.compile(r"""('''.*?'''|\"\"\".*?\"\"\"|'(?:\\.|[^'\\])*'|"(?:\\.|[^"\\])*")""")


def _outside_strings(line, fix):
    parts = _STRING.split(line)
    return "".join(p if i % 2 else fix(p) for i, p in enumerate(parts))


def _py2_line(line):
    line = _outside_strings(line, lambda s: _re.sub(r"`([^`]*)`", r"repr(\1)", s.replace("<>", "!=")))
    m = _re.match(r"^(\s*)print\b(?!\s*\()(.*)$", line)
    if m:
        args = m.group(2).strip().rstrip(",")
        line = "%sprint(%s)" % (m.group(1), args)
    m = _re.match(r"^(\s*except\s+)(\([^)]*\)|[\w.]+)\s*,\s*(\w+)\s*:(.*)$", line)
    if m:
        line = "%s%s as %s:%s" % m.groups()
    return line


def __ox_compile__(source, name):
    lines = _re.split(r"\r\n|\r|\n", source)
    body = "def %s(value, cell, cells, row, rowIndex, value1, value2):" % name + "".join("\n  " + _py2_line(l) for l in lines)
    tree = _ast.fix_missing_locations(_Py2().visit(_ast.parse(body)))
    scope = dict(_PY2)
    exec(compile(tree, "<expression>", "exec"), scope)
    return scope[name]


class _Missing(Exception):
    pass


class _Fields(object):
    """cell, cells and row as Jython sees them: item and attribute access, KeyError/AttributeError when absent."""

    def __init__(self, cls, get):
        object.__setattr__(self, "_cls", cls)
        object.__setattr__(self, "_get", get)

    def _field(self, name):
        if self._get is None:
            raise _builtins.Exception("java.lang.NullPointerException: java.lang.NullPointerException")
        return self._get(name)

    def __getitem__(self, name):
        v = self._field(name)
        if v is None:
            raise KeyError(name)
        return v

    def __getattr__(self, name):
        if name.startswith("__"):
            raise AttributeError(name)
        v = self._field(name)
        if v is None:
            raise AttributeError("'com.google.refine.jython.JythonHasFieldsWrapper' object has no attribute '%s'" % name)
        return v

    def __repr__(self):
        return "[object %s]" % self._cls


def _cell(value, recon):
    if value is None and recon is None:
        return None
    return _Fields("Cell", {"value": value, "recon": recon}.get)


def _value(v):
    if isinstance(v, tuple) and v and v[0] == "date":
        return _datetime.datetime(*v[1:])
    return v


def __ox_call__(fn, value, current, columns, cells, index, starred, flagged):
    """Call a compiled expression; `cells` is [(value, recon)] by column, or None when unused."""
    try:
        value = _value(value)
        cell = _Fields("Cell", None) if current is None else _cell(value, current[1])
        if cell is None:
            cell = _Fields("Cell", None)
        tuple_ = None
        row = None
        if cells is not None:
            by_name = dict((n, _cell(_value(c[0]), c[1])) for n, c in zip(columns, cells))
            tuple_ = _Fields("CellTuple", by_name.get)
            row = _Fields("Row", {"index": index, "starred": starred, "flagged": flagged, "cells": tuple_}.get)
        return __ox_unwrap__(fn(value, cell, tuple_, row, index, None, None))
    except _builtins.BaseException as e:
        message = str(e)
        if isinstance(e, KeyError) and e.args:
            message = repr(e.args[0])
        return ("err", "%s: %s" % (type(e).__name__, message) if not message.startswith("java.") else message)


_LONG = (-(2 ** 63), 2 ** 63)


def __ox_unwrap__(o):
    if o is None:
        return None
    if isinstance(o, bool):
        return int(o)
    if isinstance(o, int):
        if _LONG[0] <= o < _LONG[1]:
            return o
        return ("err", "OverflowError: long int too large to convert")
    if isinstance(o, float):
        return o
    if isinstance(o, str):
        return o
    if isinstance(o, (bytes, bytearray)):
        return bytes(o).decode("latin-1")
    if isinstance(o, _datetime.datetime):
        if o.tzinfo is not None:
            o = o.astimezone(_datetime.timezone.utc).replace(tzinfo=None)
        return ("date", o.year, o.month, o.day, o.hour, o.minute, o.second, o.microsecond)
    if isinstance(o, _Fields):
        if o._cls == "Cell" and o._get is not None:
            return ("cell", __ox_unwrap__(o._get("value")))
        return ("obj", o._cls)
    if isinstance(o, dict):
        return ("opaque", repr(o))
    if isinstance(o, (set, frozenset)):
        return ("opaque", "set([%s])" % ", ".join(repr(x) for x in o))
    if isinstance(o, (list, tuple, range)) or hasattr(o, "__iter__"):
        return [__ox_unwrap__(x) for x in o]
    if isinstance(o, _numbers.Number):
        return float(o)
    return ("opaque", repr(o))


# ---------------------------------------------------------------- extensions
# An extension is a folder with main.py that registers GREL functions, importers, exporters and
# operations through the `oxrefine` module. See docs/src/extensions.md.
import json as _json
import sys as _sys
import types as _types

_EXT = {}
_loading = [None]


def _register(kind, key, meta):
    if not isinstance(key, str) or not key:
        raise ValueError("an extension %s needs a name" % kind)

    def deco(f):
        _EXT[(kind, key)] = (f, dict(meta, kind=kind, id=key, owner=_loading[0]))
        return f

    return deco


_api = _types.ModuleType("oxrefine")
_api.__doc__ = "Register OxRefine extension points. See docs/src/extensions.md."
_api.function = lambda name, description="", params="": _register("function", name, {"description": description, "params": params})
_api.importer = lambda id, label=None, extensions=(): _register("importer", id, {"label": label or id, "extensions": [e.lower().lstrip(".") for e in extensions]})
_api.exporter = lambda id, label=None, extension="txt", mime="text/plain": _register("exporter", id, {"label": label or id, "extension": extension.lstrip("."), "mime": mime})
_api.operation = lambda id, label=None, description="", params=(): _register("operation", id, {"label": label or id, "description": description, "params": [dict(p) if isinstance(p, dict) else {"name": p} for p in params]})
_sys.modules["oxrefine"] = _api


def __ox_load_ext__(name, source, path):
    """Run an extension's main.py; returns what it registered, as JSON."""
    for k in [k for k, (f, m) in _EXT.items() if m["owner"] == name]:
        del _EXT[k]
    _loading[0] = name
    try:
        exec(compile(source, path, "exec"), {"__name__": "oxrefine_ext_" + name.replace("-", "_"), "__file__": path})
    finally:
        _loading[0] = None
    return _json.dumps([m for (f, m) in _EXT.values() if m["owner"] == name])


def _ext(kind, key):
    try:
        return _EXT[(kind, key)][0]
    except KeyError:
        raise LookupError("no extension %s named %s" % (kind, key))


def _values(xs):
    return [_value(x) for x in xs]


def __ox_ext_function__(name, args):
    try:
        return __ox_unwrap__(_ext("function", name)(*_values(args)))
    except _builtins.Exception as e:
        return ("err", "%s: %s" % (type(e).__name__, e))


def _table(result, what):
    if isinstance(result, dict):
        columns, rows = result.get("columns"), result.get("rows")
    else:
        columns, rows = result
    columns = [str(c) for c in columns]
    rows = [[__ox_unwrap__(v) for v in r] for r in rows]
    for i, r in enumerate(rows):
        if len(r) > len(columns):
            raise ValueError("%s: row %d has %d cells for %d columns" % (what, i + 1, len(r), len(columns)))
    return (columns, rows)


def __ox_ext_import__(id, data, options):
    return _table(_ext("importer", id)(bytes(data), _json.loads(options)), "importer " + id)


def __ox_ext_export__(id, columns, rows, options):
    out = _ext("exporter", id)(list(columns), [_values(r) for r in rows], _json.loads(options))
    return out.encode("utf-8") if isinstance(out, str) else bytes(out)


def __ox_ext_operation__(id, columns, rows, params):
    return _table(_ext("operation", id)(list(columns), [_values(r) for r in rows], _json.loads(params)), "operation " + id)
