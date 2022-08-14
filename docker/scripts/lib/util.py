import typing as t
import json


def pretty_print(obj: t.Any) -> None:
    if isinstance(obj, str):
        print(obj)
    else:
        print(json.dumps(obj, indent=1))


def read_bytes(filename: str) -> bytes:
    with open(filename, "rb") as f:
        return f.read()
