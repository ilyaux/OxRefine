// node benchmarks/gen.mjs [rows] > benchmarks/data/people.csv — messy, reproducible test data.
const n = +(process.argv[2] ?? 1_000_000);
let seed = 42;
const rnd = (k) => Math.floor(((seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648) * k);
const pick = (a) => a[rnd(a.length)];
const first = ["James", "Mary", "José", "Anna", "Li", "Olga", "Ahmed", "Chloé", "Ivan", "Sara", "Noah", "Emma"];
const last = ["Smith", "García", "Müller", "Ivanova", "Chen", "Okafor", "Rossi", "Dubois", "Kim", "Novak"];
const cities = ["New York", "Los Angeles", "São Paulo", "Zürich", "Saint Petersburg", "Mexico City", "Köln", "Kraków"];
const mess = [(s) => s, (s) => s, (s) => s.toLowerCase(), (s) => s.toUpperCase(), (s) => ` ${s}  `,
  (s) => s.replace(" ", "-"), (s) => s.normalize("NFD").replace(/\p{M}/gu, ""), (s) => s.replace(" ", "  ")];
const q = (s) => (/[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s);
const out = ["id,name,city,country,email,amount,signup"];
for (let i = 1; i <= n; i++) {
  const f = pick(first), l = pick(last), c = pick(cities);
  out.push([i, pick(mess)(`${f} ${l}`), pick(mess)(c), pick(["US", "us", "USA", "United States", "DE", "Germany", "BR", "Brasil"]),
    `${f}.${l}${rnd(1000)}@example.com`.toLowerCase(), (rnd(100000) / 100).toFixed(2),
    `20${10 + rnd(15)}-${String(1 + rnd(12)).padStart(2, "0")}-${String(1 + rnd(28)).padStart(2, "0")}`].map(q).join(","));
  if (out.length === 50000) { process.stdout.write(out.join("\n") + "\n"); out.length = 0; }
}
process.stdout.write(out.length ? out.join("\n") + "\n" : "");
