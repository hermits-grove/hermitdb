# gitdb

###### A privacy concious database for apps that respect user privacy 

#### Should you use gitdb?

Gitdb is not fast and it's features and tooling are bare bones, but... it may be enough for most apps.

Gitdb's goals is to be a fast-enough, offline-first, distributed key-value store with strong privacy and support for automated conflict resolution.

Applications built using GitDB tap into a large federated network for storing data: Git is a widely deployed and well understood protocol, users have a wide selection of storage options to choose from. They may pay Github, Gitlab, Bitbucket, etc.. or even host their own. 

The original motivator for gitdb was *mona,* a password manager which relied on Git to manage state across devices (*[the project is in development here.](https://github.com/the-gitdb-cooperative/mona)*)

*You should consider using gitdb if you care for user agancy over their data **and** you do not need to store obscene amounts of data or perform 100s of operations per second*

#### Design

GitDB stores all mutations to the database in git commits. These mutations are called `Ops` and they allow us to replicate the minimal amount of information necessary for other devices to stay in sync.

Everytime a mutating operation is performed on the database, we generate an `Op`. Op's themselves are also CRDT's meaning we can merge two Ops into a new Op which describe the mutations of both Ops.

This is done to avoid generating an excessive number of commits. The cached `Op` is committed only when we push to the git remote.

##### Op

``` javascript
// example remove op
{
  "dot": {
      "actor": base64(98), // Actor chosen randomly, 128 bits to avoid collision
      "version: 33
  },
  "op": {
    "action": "remove",
    "key": base64("xyz".to_bytes()), // no key encoding restrictions, just bytes
    "type": "map"
  }
}

{
  "action": "remove",
  "key": base64("xyz".to_bytes()),                                     r
  "type": "map", // key + type identify an entry, `type` avoids merge conflict
  "dot": {
    "actor": base64(98),
    "version": 32 // u64 actor version number
  }
}

// Example Update op which sets "autologoff" to `false`.
//
// "autologoff" is an entry in the `prefs` map which, in turn, is an
// entry in the `mona` map.
{
  "dot": {
    "actor": base64(98),
    "version: 33
  },
  "op": {
    "action": "update",
    "key": base64("mona".to_bytes()),
    "type": "map",
    "op": { // Values are also CRDT's, nested op's structure depends on `type`
      "action": "update",
      "key": base64("pref".to_bytes()),
      "type": "map",
      "op": {
        "action": "update",
        "key": base64("autologoff".to_bytes()),
        "type": "reg",
        "op": {
          "val": { // TAI: registers can hold any type of data?
            "type": "bool",
            "data": false
          }
        }
      }
    }
  }
}
```

## Prior Art

- https://github.com/ff-notes/ff - a distributed notes app built with CRDT's + Git
