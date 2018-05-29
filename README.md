# gitdb

###### A privacy concious database for apps that respect user privacy 

#### Should you use gitdb?

 Gitdb is not fast and it's features and tooling are bare bones, but... it may be just enough.

Gitdb's goals is to be a fast-enough, offline-first, distributed key-value store with strong confidentiality and support for automated conflict resolution.

Applications built using gitdb tap into a large federated network for storing data: Git is a widely deployed and well understood protocol, technical users have a wide selection of storage options to choose from, they may pay Github, Gitlab, Bitbucket, etc.. or even host their own Git server. 

The original motivator for gitdb was *mona,* a password manager which relied on Git to manage state across devices (*[the project is in development here.](https://github.com/the-gitdb-cooperative/mona)*)

*You should consider using gitdb if you care for user agancy over their data **and** you do not need to store obscene amounts of data / perform 100s of operations per second*

## Design

The main abstraction in gitdb is the `Block` datatype, these are primitive CRDT's gitdb understands and can merge automatically. A `Block` is stored in gitdb as a single file on disk.

A `Block` stored in gitdb is uniquely referenced by a `key`. A `key` can be any utf-8 string.

###### Mapping a key to a file on disk

Often metadata, like a block key, can be just as incriminating as the data stored in a block. To avoid leaking any sensitive information gitdb derives an obfuscated filepath from a key.

The filepath derivation algorithm is outlined here

```haskell
key <- "mona#accounts#news.ycombinator.com#davidrusu@pass"
key_salt <- decrypt(read_file("$GITDB_ROOT/key_salt"))
hash <- sha256(key_salt ++ path)
dir_part, file_part <- hash.split_at(2)
filepath <- path.join("$GIT_ROOT", "cryptic", dir_part, file_part)
-- => something like $GIT_ROOT/cryptic/d6/1f774e6...a97bde0b87a
```

An encrypted key salt (256 bits of random) is stored at the root of the Git repository `./key_salt`.

### Crypto

#### TAI: Safe Nonce Use

- designate first 32 bits of nonce to site_id
- per site 64 bit nonce stored in `./sites/<site_id>/NONCE`

`96 bit nonce := [ site_id: u32 | site_nonce: u64]`

`site_nonce` must be incremented and written to disk after each use.

#### Avoiding Nonce Reuse Attacks (by not using nonces)

*WORKAROUND UNTIL https://github.com/briansmith/ring/issues/411 IS RESOLVED*

As described in https://tools.ietf.org/html/rfc5116#section-3.1 

> Many problems with nonce reuse can be avoided by changing a key in a situation in which nonce coordination is difficult.

Since we are using an encryption algorithm who's nonce is 96bits, the nonce space is not large enough to give us confidence in random nonces.

Instead we use randomly generated 256bit salts as inputs to our kdf to give us unique encryption keys each time we encrypt. Salts are never reused.

Why is this done? Protecting against nonces reuse in a distributed system is difficult, for instance if we use naive incrementing nonces, we could enter a situation where two sites both modify and re-encrypt the same block: both sites would increment the same nonce but they are encrypting (potentially) different plaintext, if we are not careful how we resolve this conflict we will expose the secret key.

#### entropy files

Entropy files are random 256 bit keys  that are used to add additional entropy to the key derivation process. They are stored in plaintext on each device.

The same entropy file must be present on each site to access your data.

##### Protocol for entropy_file Key Exchange

New site makes clones the git repository

**Case:** no entries in <root>/db/sites/
Either this is the first site added to this gitdb instance or other site has not synchronized yet with the remote.

In either case, generate a new entropy_file, key pair, store files at `<root>/entropy_file`, `<root>/db/sites/<site_id>/id.pub` and `<root>/id.priv` respectively.

Attempt to push

If push fails, this means another site has created a entropy_file before you did, delete your entropy_file and start over.

**Case:** there exists entries in `<root>/db/sites/*`
In this case a entropy_file already exists

generate a key pair, store files at `<root>/db/sites/<site_id>/id.pub` and `<root>/id.priv` and synchronize.

periodically run `git pull` followed by checking if `<root>/db/sites/<site_id>/entropy_file.<existing_site_id>` exists.

*as an existing site*

Sites are expected to periodically sync gitdb with remotes.

on sync, sites detect newly added sites by scanning `<root>/db/sites/` for entries which are missing `<root>/db/sites/<some_site_id>/entropy_file.*`.

Once a new site is detected, gitdb will prompt user for decision of whether they trust this new site. This is done through a callback passed to sync.

On confirmation from user, this site will:
1. generate an ephemeral key using the new site's public key
2. encrypt the entropy_file using this ephemeral key
3. write the encrypted entropy_file to `<root>/db/sites/<new_site_id>/entropy_file.<site_id>`

And proceed with sync.

*back to new site*

on next `git pull`, we should have `<root>/db/sites/<site_id>/entropy_file.<existing_site_id>`

Notify user which site gave us the entropy_file. Ask for confirmation that they trust this site.

On confirmation, read the file and decrypt:
1. generate an ephemeral key using the new site's public key
2. encrypt the entropy_file using this ephemeral key
3. write the encrypted entropy_file to `<root>/db/sites/<new_site_id>/entropy_file.<site_id>`

Despite user confirmation, authenticity of entropy_file must be verified by attempt to decrypt `<root>/db/path_salt`. An attacker with access to the remote may have created the `db/sites/<site_id>/entropy_file` maliciously, if we don't attempt to decrypt an encrypted file with this entropy_file, we risk making ourselves susceptible to bruteforce against the master passphrase.

write plaintext entropy_file to `<root>/entropy_file`

#### Key Derivation

###### key material

**entropy_file:** random 256 bits stored in plaintext on each site, keep this hidden from any third party.

**master_passphrase:** strong user chosen passphrase.

**block_salt:** randomly generated salt per block. Protects us from nonce reuse and input to pbkdf2.

**block_iters:** input to pbkdf2

###### kdf algorithm

```haskell
INPUT: key_file          -- 256b key file from site (not stored in GitDB)
INPUT: master_passphrase -- read from users mind
INPUT: block_salt        -- random salt per block
INPUT: block_iters       -- extracted from Block

pbkdf2_key <- PBKDF2(
  algo: SHA_256,
  pass: master_passphrase,
  salt: block_salt,
  iters: block_iters,
  length: 256
)

key <- pbkdf2_key XOR key_file

OUTPUT: key
```

#### Encryption

```haskell
INPUT: key_file          -- 256b key file from site (not stored in GitDB)
INPUT: master_passphrase -- read from users mind
INPUT: plaintext         -- plaintext data to encrypt

block_salt <- rand(256)  -- random 256bit salt
block_iters <- 1000000   -- u32 read from config

-- See above for kdf algorithm
key <- kdf(key_file, master_passphrase, block_salt, block_iters)

ciphertext <- AEAD_encrypt(
  algo: CHACHA20_POLY1305
  secret_key: key,
  nonce: 0, -- nonce not used, see above section on nonce reuse
  ad: block_salt ++ block_iters
)

block <- block_iters ++ block_salt ++ ciphertext
--     | 32bit uint  |  256bit salt | <n>bit ciphertext |       

OUTPUT: block
```



#### Decryption

```haskell
INPUT: key_file          -- 256b key file from site (not stored in GitDB)
INPUT: master_passphrase -- read from users mind
INPUT: block             -- block to decrypt

block_iters <- block[0..32]
block_salt <- block[32..(32 + 256)]
ciphertext <- block[(32+256)..block.len()]

-- See above for kdf algorithm
key <- kdf(key_file, master_passphrase, block_salt, block_iters)

plaintext <- AEAD_decrypt(
  algo: CHACHA20_POLY1305
  secret_key: key,
  nonce: 0, -- nonce not used, see above section on nonce reuse
  ad: block_salt ++ block_iters
)

OUTPUT: plaintext
```



## Automating Merge Conflicts

GitDB is meant to be used in an offline first context, this necessarily means that conflicts are bound to happen and managing these conflicts in an unintruisive way is paramount to building useful applications on top of GitDB.

GitDB leans heavily on the fantastic [Ditto](https://github.com/alex-shapiro/ditto) collection of CRDT data structures. As long as you only use Ditto structures, conflicts will be handled automatically for you.

Unfortunately, Ditto does not help us in choosing a site identifier

### Assigning Site ID's

Random u32 checked against existing site identifiers, that's all we've got right now.

This gives us a probability of collision between two new offline devices at a bit over $10^{-10}$. Not great, but it'll do for now.

### Merge Procedure

#### Remote Ordering

The order of pushing to remotes is meaningfully, if clients don't all iterate in the same order there is potential for remotes to never converge to each other.

For example consider a situation with two sites and two remotes and both sites choose different first push remotes. In the case where both sites differ in commit history, you will enter a situation of repeated indefinite merging and pushing (assuming the worst case where operations are happening in lockstep and merges by sites differ in some way).

To avoid this, we iterate remotes in the same order on each site, on conflict, we stop iterating, merge the conflict and restart the push iteration from the beginning.

In pseudo code:

```rust
let repo = git.open_repo("$GIT_ROOT");

let push_succeded = false;
while !push_succeeded {
    push_succeeded = true;
    for remote in repo.remotes.sort().iter() {
        remote.fetch();
        repo.merge(remote.refspec())
        if repo.index.has_conflict() {
            // <handle_conflict>
            push_succeeded = false;
            break;
        }
    }
}
```

#### Conflict Resolution Semantics and Limitations

Merge semantics of JSON CRDT's come from the underlying `ditto` library


But the filesystem tree structure is managed by gitdb

We'll need to deal with a few cases:
(notation: `<action> <id>:<path>` ~= Site <id> took <action> on data at <path>)

**case**
modified `A:/a`
modified `B:/a`

merge: handled by ditto `ditto::merge(db.get("A:/a"), db.get("B:/b"))`

**case**
modified `A:/a`
deleted `B:/a`

merge: keep `A:/a`

**case**
modified `A:/a/b/c`
deleted `B:/a`

merge: keep `A:/a/b/c` delete everything else under `/a/*`

**case**
created `A:/foo`
created `B:/foo`

merge:
    if `A:/foo` and `B:/foo` are of the same crdt type
        `ditto::merge("A:/foo", "B:/foo")`
    otherwise merge fails

# Known Failure Modes

## Super bad catastrophic crypto failure, fix this!!

SiteId's are u32's generated at random.
They are used to resolve merge's in datatypes
and used to generate unique nonces

32bits is tiny! theres a chance of two sites ending up with the same `site_id`. When this happens we may expose our secret key:

new site A with `site_id = 472`
new site B with `site_id = 472`

both sites encrypt data using the same `site_nonce = [472u32 | 0u64]`

`A:/foo`
`B:/bar`

once they push to remotes, confidentiality will be lost, the encryption key may be exposed

*Potential data loss*

If two sites with same SiteId's both modify the same CRDT, data loss will likely happen

How to fix?

1. IDEAL: Do some calculations on probabilities, the risk may be acceptable.

2. NOT IDEAL: Wait until first sync before allowing writes, prompt user for verification that all site-id's are present before generating a random site-id and checking against existing sites.

3. .... still thinking

#### Prior Art

- https://github.com/ff-notes/ff - a distributed notes app built with CRDT's + Git
- 