# GitDB - Security First Datastore for User Focused Applications

GitDB aims to provide an encrypted at rest distributed database for apps which help users organize their life, think todo apps, password managers, contacts manager, quantified self applications.

We want to give users control over their data while maintaining the kinds of cross device syncing we have become accustomed to with centralized applications.

GitDB uses Git as the foundation for a distributed database, by using git we inherit all of the existing infrastructure built around managing git repositories (github, gitlab, etc.)

GitDB provides a set of data structures which attempt to make conflict resolution as painless as possible.

## Design

GitDB is structured like a filesystem: the building blocks you have to work with are `keys`, `namespaces` and `blobs`.

### anatomy of a Key:

keys are checked against the regex: `^/[_\.\-a-zA-Z0-9]+(/[_\.\-a-zA-Z0-9]+)*$`

in english:
- keys start with `/`
- keys can have multiple non-empty key components using alpha-numeric, `_`, `-`, or `.` characters
- key components are seperated by `/`

Examples:
```
VALID: /mona/social/news.ycombinator.com
VALID: /aA0_-
VALID: /a/A/0/_/-
VALID: /
INVALID: /a/
INVALID: /a//b
INVALID: /#
```

### Blob
Blobs are where your data is stored.

Blob keys must not already be used to reference a namespace.

The algorithm to store a blob is outlined here:
```
blob <- encrypt(your_data)
oid <- git_add(blob)

key <- /your/namespace/data_name
/your/namespace, data_name <- partition_key(key)
ns <- read_namespace(/your/namespace)

ns.add_blob(data_name, oid)
```

The main idea is that namespaces store a reference to the git object id. Later to fetch a blob, we discover the blob by reading the namespace and following the object id.

### Namespace

Just like a blob, a Namespace is referenced by a key, the namespace in gitdb is analogous to a filesystem directory.


The algorithm to create a namespace is outlined here:
```
key <- /your/new/namespace_name
/your/new, namespace_name <- partition_key(key)
ns <- read_namespace(/your/new)

ns.add_namespace(namespace_name)
```

Namespace's store references to blobs and child namespaces.

### Analysis of GitDB actions

#### namespace

Opens the requested namespace, if it does not exist, recursively create missing namespaces along the path

```
db.namespace("/a/b")
```

Assume `/a`, `/a/b` does not exist but `/` does exist prior to call
1. `sha256("/a/b") -> ca7e59ca7c68a15c085d98ed2ec60b09354187d3c7ed8e631e82862c41eebf0c`
2. convert hash to path on disk: `./ca/7e59ca7c68a15c085d98ed2ec60b09354187d3c7ed8e631e82862c41eebf0c`
3. attempt to decrypt this file, fails since it doesn't exist
4. create empty namespace, encrypt and store on disk at `./ca/7e59ca7c68a15c085d98ed2ec60b09354187d3c7ed8e631e82862c41eebf0c`
5. recursively open namespace `/a`: `let mut parent = db.namespace("/a");`
6. add namespace entry `b` to `/a` namespace: `parent.add_entry("b", NS);`

Git repository state after call:
```
modified: ./f4/65c3739385890c221dff1a05e578c6cae0d0430e46996d319db7439f884336 // derived from sha256("/")
new file: ./2d/cc5f529a273b6c724045ba06f40c4cfd82a940615ca7de15535ca68da5dbb0 // derived from sha256("/a")
new file: ./ca/7e59ca7c68a15c085d98ed2ec60b09354187d3c7ed8e631e82862c41eebf0c // derived from sha256("/a/b")
```

#### `put`

``` rust
db.put("/a/b/c", <blob>);
```

Assume namespace `/a/b` exists prior to call
1. open namespace `/a/b`: `let mut ns = db.namespace("/a/b");` see `namespace` above
2. add <blob> to git, git returns us an `OID` (object id)
3. add blob entry `c` with oid `OID` to `/a/b` namespace: `ns.add_entry("c", BLOB)`;

Git repository state after call:
```
new blob with `OID` stored in .git
modified: ./ca/7e59ca7c68a15c085d98ed2ec60b09354187d3c7ed8e631e82862c41eebf0c // derived from sha256("/a/b")
```

#### `get`

``` rust
db.get("/a/b/c")
```

1. open namespace `/a/b`: `let ns = db.namespace("/a/b");` see `namespace` above
2. scan namespace for `c` entry: `let entry = ns.get_entry("c");`
3. ensure entry is of blob type: `entry.type == BLOB`
4. fetch the git blob using `entry.oid`
5. decrypt the blob, return plaintext

Git repository is unchanged after call



#### `rm`

``` rust
db.rm("/a/b/c");
```

Assume namespace `/a/b` exists prior to call
1. open namespace `/a/b`: `let mut ns = db.namespace("/a/b");` see `namespace` above
2. scan namespace for `c` entry: `let entry = ns.get_entry("c");`
3. ensure entry is of blob type: `entry.type == BLOB`
4. remove `c` entry from  namespace: `ns.rm_entry("c");`

Git repository state after call:
```
modified: ./ca/7e59ca7c68a15c085d98ed2ec60b09354187d3c7ed8e631e82862c41eebf0c // derived from sha256("/a/b")
```
