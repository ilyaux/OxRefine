"""Create a new disposable SQLite equivalent of the public 100k-row SQL fixtures."""
import pathlib
import sqlite3
import sys


def create(path):
    path = pathlib.Path(path)
    if path.exists():
        raise FileExistsError("Refusing to overwrite an existing database")
    path.parent.mkdir(parents=True, exist_ok=True)
    # Exclusive creation prevents accidentally replacing a file created after the check.
    with path.open("xb"):
        pass
    with sqlite3.connect(path) as connection:
        connection.execute("CREATE TABLE db_bench_fixture(id INTEGER PRIMARY KEY, label VARCHAR(40) NOT NULL, quantity INTEGER NULL)")
        connection.executemany("INSERT INTO db_bench_fixture VALUES(?,?,?)", (
            (i, f"item-{i}", None if i % 97 == 0 else i % 1000) for i in range(100_000)
        ))
        assert connection.execute("SELECT COUNT(*), SUM(quantity IS NULL), MIN(id), MAX(id) FROM db_bench_fixture").fetchone() == (100_000, 1031, 0, 99_999)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: python benchmarks/database-fixture.py <new-fixture.sqlite>")
    create(sys.argv[1])
