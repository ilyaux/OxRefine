# Clustering

**Edit cells → Cluster and edit…** finds groups of different values that probably mean the same thing, and merges each group into one value you choose.

![Clustering the city column](images/cluster.png)

## Methods

**Key collision** turns each value into a key, and values with the same key form a cluster. The keying functions are OpenRefine's:

- **fingerprint** ignores case, accents, punctuation, word order and extra spaces;
- **n-gram fingerprint** does the same on character n-grams, which catches typos inside words;
- **metaphone3**, **cologne phonetic**, **Daitch-Mokotoff** and **Beider-Morse** group values that sound alike.

**Nearest neighbours** groups values within a distance of each other, using **Levenshtein** edit distance. The *radius* sets how far apart values can be, and the *block size* speeds up the search on large columns. The PPM distance isn't available yet.

## Working through clusters

- Each cluster lists its values with their row counts. The most common value is suggested as the new one.
- Tick the clusters to merge and edit the new value if needed. *Merge and re-cluster* keeps the dialog open, *Merge and close* finishes.
- Filters narrow the list by cluster size, rows in the cluster, value length and length variance.
- *Copy clusters as JSON* and *Export clusters* save the clusters for review elsewhere.

Each merge is one mass-edit step in the history, the same operation OpenRefine records.
