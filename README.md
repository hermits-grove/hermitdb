# gitdb

###### A privacy concious database for apps that respect user privacy 



#### Should you use gitdb?

Gitdb is not fast and it's features and tooling are bare bones, but... it may be enough for most apps.

Gitdb's goals is to be a fast-enough, offline-first, distributed key-value store with strong privacy and support for automated conflict resolution.

Applications built using GitDB tap into a large federated network for storing data: Git is a widely deployed and well understood protocol, users have a wide selection of storage options to choose from. They may pay Github, Gitlab, Bitbucket, etc.. or even host their own. 

The original motivator for gitdb was *mona,* a password manager which relied on Git to manage state across devices (*[the project is in development here.](https://github.com/the-gitdb-cooperative/mona)*)

*You should consider using gitdb if you care for user agancy over their data **and** you do not need to store obscene amounts of data or perform 100s of operations per second*

## Prior Art

- https://github.com/ff-notes/ff - a distributed notes app built with CRDT's + Git
