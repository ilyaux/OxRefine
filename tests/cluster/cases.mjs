// Clustering conformance catalog. gen.mjs imports VALUES as one column "v" into OpenRefine 3.10.1,
// records GREL phonetic() for every value under every encoder, and the clusters every clusterer finds.

/** Messy values with near-duplicates; repeats give counts. Blank lines import as null and are skipped. */
export const VALUES = [
  "John Smith", "John Smith", "Jon Smyth", "Smith, John", "john smith", "JOHN SMITH", "J. Smith", "Jonathan Smith",
  "Catherine", "Katherine", "Kathryn", "Catharine", "Cathryn", "Katerina",
  "Müller", "Mueller", "Muller", "Muler", "Möller", "Miller",
  "Schmidt", "Schmitt", "Schmid", "Smith", "Smyth", "Schmidt", "Schmitz",
  "Thompson", "Thomson", "Tomson", "Thompsen",
  "MacDonald", "McDonald", "Mac Donald", "Macdonald", "O'Brien", "OBrien", "O Brien", "Obrien",
  "Jürgen", "Jurgen", "Juergen", "Yurgen",
  "Đorđe", "Djordje", "Ørsted", "Orsted", "Æsir",
  "Zhang Wei", "Wei Zhang", "Chang Wei",
  "Alexander", "Aleksandr", "Alexandre", "Alejandro", "Sasha",
  "New York", "new york", "New-York", "NewYork", "New York City", "NYC", "Newark",
  "San Francisco", "San Fransisco", "SanFrancisco", "Santa Fe",
  "Acme Inc.", "ACME Inc", "Acme, Inc.", "Acme Incorporated", "Acme Corp", "Acne Inc",
  "International Business Machines", "IBM", "I.B.M.",
  "Coca-Cola", "Coca Cola", "CocaCola", "Coke",
  "colour", "color", "colours", "honour", "honor",
  "123", "123.0", "1 2 3", "---", "...", "a", "A", "ab", "ba",
  "Mississippi", "Missisippi", "Misissippi", "Mississipi",
  "Washington", "Washingtn", "Wahsington",
  "Ivanov", "Ivanoff", "Iwanow", "Ivanova",
  "Dvořák", "Dvorak", "Dvorack",
  "Ngozi", "Nkosi", "Gnosis",
  "Philip", "Phillip", "Filip", "Felipe",
  "Muñoz", "Munoz", "Peña", "Pena", "Gonçalves", "Goncalves", "Çelik", "Celik", "Ñandú", "Façade",
  "", "  padded  ", "padded",
]

export const ENCODINGS = ["metaphone3", "doublemetaphone", "metaphone", "soundex", "cologne-phonetic", "daitch-mokotoff", "beider-morse", "fingerprint", "ngram-fingerprint"]

const bin = (fn, params = {}) => ({ type: "binning", function: fn, column: "v", params })
const knn = (fn, radius, block) => ({ type: "knn", function: fn, column: "v", params: { radius, "blocking-ngram-size": block } })

export const CLUSTERERS = [
  bin("fingerprint"),
  bin("ngram-fingerprint", { "ngram-size": 1 }),
  bin("ngram-fingerprint", { "ngram-size": 2 }),
  bin("ngram-fingerprint", { "ngram-size": 3 }),
  bin("metaphone3"),
  bin("cologne-phonetic"),
  bin("daitch-mokotoff"),
  bin("beider-morse"),
  knn("levenshtein", 1, 6),
  knn("levenshtein", 1, 2),
  knn("levenshtein", 2, 3),
  knn("levenshtein", 3, 2),
  knn("ppm", 1, 6),
  knn("ppm", 2, 3),
  knn("ppm", 4, 2),
  knn("ppm", 0.5, 4),
]
