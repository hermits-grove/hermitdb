# gitdb

###### A privacy concious database for apps that respect user privacy 

#### Should you use gitdb?

Gitdb is not fast and it's features and tooling are bare bones, but... it may be just enough.

Gitdb's goals is to be a fast-enough, offline-first, distributed key-value store with strong confidentiality and support for automated conflict resolution.

Applications built using gitdb tap into a large federated network for storing data: Git is a widely deployed and well understood protocol, technical users have a wide selection of storage options to choose from, they may pay Github, Gitlab, Bitbucket, etc.. or even host their own Git server. 

The original motivator for gitdb was *mona,* a password manager which relied on Git to manage state across devices (*[the project is in development here.](https://github.com/the-gitdb-cooperative/mona)*)

*You should consider using gitdb if you care for user agancy over their data **and** you do not need to store obscene amounts of data or perform 100s of operations per second*

## Design

Gitdb is a key-value store, where key's are any valid utf8 string and key's are CRDT's.

For gitdb to work, it needs to understand how to merge conflicts. Gitdb provides a set of CRDT's which can be used as building blocks for applications to develop apps that converge to a well defined state.

Gitdb calls these CRDT's a `Block` and will automate the syncing and merging of these blocks with the Git protocol.

#### Writing a new block

```
db.write_block("users#john@age", &Block::Val(Prim::F64(34.12), &sess);
```

Gitdb will:

- wrap the `Block` in a `Register` CRDT
  - (A Register CRDT is a CRDT that holds a value)
- serialize the register
- encrypt the serialized  register using the `Session` passed in
- map the key `users#john@age` to a path on disk
- write the ciphertext to derived path on disk
- add the path to the git index

#### Overwriting an existing block

```
db.write_block("users#john@age", &Block::Val(Prim::F64(35.3), &sess);
```

Gitdb will:

- map the key `users#john@age` to a path on disk
- notice this path points to a file and read the file from disk.
- decrypt the file contents using the `Session` passed in.
- deserialize the Register-wrapping-old-Block from the plaintext data
- `update` the Register to write store the new Block.
- serialize the Register-wrapping-new-Block
- encrypt the serialized  register using the `Session` passed in
- write the ciphertext to the same path on disk
- add the path to the git index



#### Merging / Sync

Gitdb does not commit changes to Git until `db.sync` is called. This is done to minimize to the Git overhead when writing data.

```
db.sync(&sess);
```

Gitdb will:

- Commit the working index
- pull latest changes from remote
- merge any conflicting files by:
  - `decrypt(old); decrypt(new)`
  - deserialize both as Registers-holding-Blocks
  - get Blocks from old and new registers and attempt to merge them.
  - If successful, update old register to hold the merged Block.
  - If not successful, merge the two registers
  - serialize the resulting register, encrypt and write to disk

##### Mapping a key to a file on disk

Often metadata, like a block key, can be just as incriminating as the data stored in a block. To avoid leaking any sensitive information gitdb derives an obfuscated filepath from a key.

The key-to-filepath tranformation is outlined here

```haskell
key <- "mona#accounts#news.ycombinator.com#davidrusu@pass"
key_salt <- decrypt(read_file("$GITDB_ROOT/key_salt")) -- key_salt is stored encrypted
hash <- sha256(key_salt ++ key)                        -- key_salt is mixed with key
dir, file <- string_split_at(hash, 2)                  -- avoid many files in a dir 
filepath <- path_join("$GIT_ROOT", "cryptic", dir, file)
-- => filepath ~= $GITDB_ROOT/cryptic/d6/1f774e6...a97bde0b87a
```

An encrypted key salt (256 bits of crypto grade random) is stored at the root of the Git repository:

`$GITDB_ROOT/key_salt`.

### Crypto

#### Nonce Reuse

Current state of affairs: We have a 96 bit random nonce, this gives us a key life time of $ 2^{32}$ encrypted messages before a key rotations is neccessary. This is plenty for many applications but we should be able to do better.

#### Improvements to random nonce

##### Explore: Use KDF to stretch lifetime of Key

Amazon uses a kdf to expand the life of the key: https://www.youtube.com/watch?v=WEJ451rmhk4

sketch of algorithm:

```
base_key <- /* see key derivation section */
file_salt <- rand_256()
file_key <- kdf(base_key, salt: file_salt)

OUTPUT: (file_salt, encrypt(data, file_key))
```

This gives us essentially a 256 bit nonce which should make the key lifetime practically infinite.

##### Explore: Safe Nonce Using SiteId

- designate first 32 bits of nonce to site_id
- per site 64 bit nonce stored in `./sites/<site_id>/NONCE`

`96 bit nonce := [ site_id: u32 | site_nonce: u64]`

`site_nonce` must be incremented and written to disk after each use.

##### Explore: Avoiding Nonce Reuse Attacks (by not using nonces)

As described in https://tools.ietf.org/html/rfc5116#section-3.1 

> Many problems with nonce reuse can be avoided by changing a key in a situation in which nonce coordination is difficult.

Generated a random salt per file and use that as input to the key kdf



*OR WAIT TILL https://github.com/briansmith/ring/issues/411 IS RESOLVED*

#### Entropy files

Entropy files are random 256 bits  that are used to add additional entropy to the key derivation process. They are stored in plaintext on each device.

The same entropy file must be present on each site to access your data.

##### Protocol for entropy_file Exchange

We need to have an easy way of transporting this entropy_file to new devices.

TBD probably involves some fancy asymmetric crypto...

## Prior Art

- https://github.com/ff-notes/ff - a distributed notes app built with CRDT's + Git