extern crate time;
extern crate git2;
extern crate rmp_serde;
extern crate ring;

use self::git2::{Repository, Commit};

use std::path::Path;

use error::{Result, Error};
use remote::Remote;

pub fn fetch<'a>(repo: &'a Repository, remote: &Remote) -> Result<git2::Remote<'a>> {
    eprintln!("fetching remote {}", &remote.name);

    let mut git_remote = match repo.find_remote(&remote.name) {
        Ok(git_remote) => git_remote,
        Err(e) => {
            eprintln!("find_remote failed: {:?}", e);
            // this remote is not added to git yet, we add it
            repo.remote(&remote.name, &remote.url)?
        }
    };

    let mut fetch_opt = git2::FetchOptions::new();
    fetch_opt.remote_callbacks(remote.git_callbacks());
    git_remote.fetch(&["master"], Some(&mut fetch_opt), None)?;
    Ok(git_remote)
}

pub fn commit(repo: &Repository, msg: &str, extra_parents: &[&Commit]) -> Result<()> {
    eprintln!("committing");

    let mut index = repo.index()?;
    let tree = index.write_tree()
        .and_then(|tree_oid| repo.find_tree(tree_oid))?;
    
    let parent: Option<Commit> = match repo.head() {
        Ok(head_ref) => {
            let head_oid = head_ref.target()
                .ok_or(Error::State(format!("Failed to find oid referenced by HEAD")))?;
            let head_commit = repo.find_commit(head_oid)?;
            Some(head_commit)
        },
        Err(_) => None // initial commit (no parent)
    };

    match parent {
        Some(ref commit) => {
            let prev_tree = commit.tree()?;
            let stats = repo.diff_tree_to_tree(Some(&tree), Some(&prev_tree), None)?.stats()?;
            if stats.files_changed() == 0 {
                eprintln!("aborting commit, no files changed");
                return Ok(())
            }
        },
        None => {
            if index.is_empty() {
                eprintln!("aborting commit, Index is empty, nothing to commit");
                return Ok(());
            }
        }
    }

    let sig = repo.signature()?;

    let mut parent_commits = Vec::new();
    if let Some(ref commit) = parent {
        parent_commits.push(commit)
    }
    parent_commits.extend(extra_parents);
    
    repo.commit(Some("HEAD"), &sig, &sig, &msg, &tree, &parent_commits)?;
    Ok(())
}

pub fn stage_file(repo: &Repository, file: &Path) -> Result<()> {
    let mut index = repo.index()?;
    index.add_path(&file)?;
    index.write()?;
    Ok(())
}

pub fn fast_forward(repo: &Repository, branch: &git2::Branch) -> Result<()> {
    eprintln!("fast forwarding repository to match branch {:?}", branch.name()?);
    let remote_commit_oid = branch.get().resolve()?.target()
        .ok_or(Error::State("remote ref didn't resolve to commit".into()))?;

    let remote_commit = repo.find_commit(remote_commit_oid)?;

    if let Ok(branch) = repo.find_branch("master", git2::BranchType::Local) {
        let mut branch_ref = &mut branch.into_reference();
        branch_ref.set_target(remote_commit_oid, "fast forward")?;
    } else {
        eprintln!("creating local master branch");
        repo.branch("master", &remote_commit, false)?;
    }
    repo.set_head("refs/heads/master")?;
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
    Ok(())
}

pub fn sync<'a>(repo: &Repository, remote: &Remote, mut merger: &mut (FnMut(git2::DiffDelta, f32) -> bool + 'a)) -> Result<()> {
    // we assume all files to be synced have already been added to the index
    commit(&repo, "sync commit from site", &[])?;

    // fetch and merge
    let mut git_remote = fetch(&repo, &remote)?;

    eprintln!("searching for remote master branch");
    let remote_master_ref = format!("{}/master", &remote.name);
    if let Ok(branch) = repo.find_branch(&remote_master_ref, git2::BranchType::Remote) {
        eprintln!("found remote master branch");
        let remote_commit_oid = branch.get().resolve()?.target()
            .ok_or(Error::State("remote ref didn't resolve to commit".into()))?;

        let remote_annotated_commit = repo.find_annotated_commit(remote_commit_oid)?;

        let (analysis, _) = repo.merge_analysis(&[&remote_annotated_commit])?;

        use self::git2::MergeAnalysis;
        if analysis == MergeAnalysis::ANALYSIS_NORMAL {
            let remote_commit = repo.find_commit(remote_commit_oid)?;
            let remote_tree = remote_commit.tree()?;

            // now the tricky part, detecting and handling conflicts
            // we want to merge the local tree with the remote_tree

            // TODO: see if there are any diff options we can use to speed up the diff
            let diff = repo.diff_tree_to_index(Some(&remote_tree), None, None)?;
            eprintln!("iterating foreach");
            diff.foreach(&mut merger, None, None, None)?;
            commit(&repo, "merge commit", &[&remote_commit])?;
        } else if analysis.contains(MergeAnalysis::ANALYSIS_FASTFORWARD) {
            fast_forward(&repo, &branch)?;
        } else if analysis == git2::MergeAnalysis::ANALYSIS_UP_TO_DATE {
            eprintln!("nothing to merge, ahead of remote");
        } else {
            return Err(Error::State(format!("Bad merge analysis result: {:?}", analysis)));
        }
    }
    
    eprintln!("pushing git_remote");
    let mut push_opt = git2::PushOptions::new();
    push_opt.remote_callbacks(remote.git_callbacks());
    git_remote.push(&[&"refs/heads/master"], Some(&mut push_opt))?;
    eprintln!("Finish push");
    
    // TAI: should return stats struct
    Ok(())
}
