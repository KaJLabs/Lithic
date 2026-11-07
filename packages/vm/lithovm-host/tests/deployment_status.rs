use lithovm_host::deployment_status::{included, Inclusion};
use lithovm_host::{
    child_contract_address, contract_address, CommittedDeployment, DeploymentOrigin,
};

fn word(n: u8) -> [u8; 32] {
    let mut word = [0; 32];
    word[31] = n;
    word
}
fn context() -> Inclusion {
    Inclusion {
        chain_id: 700777,
        transaction_hash: word(1),
        block_hash: word(2),
        block_height: u64::MAX,
        transaction_gas_used: u64::MAX,
    }
}
fn records() -> Vec<CommittedDeployment> {
    let factory = contract_address(word(9), 1, word(3), 700777);
    vec![
        CommittedDeployment {
            creator: word(9),
            contract: factory,
            code_hash: word(3),
            origin: DeploymentOrigin::Transaction { nonce: 1 },
        },
        CommittedDeployment {
            creator: factory,
            contract: child_contract_address(factory, word(4), word(5), 700777),
            code_hash: word(5),
            origin: DeploymentOrigin::Child {
                template: word(6),
                salt: word(4),
            },
        },
    ]
}
#[test]
fn registration_status_preserves_precision_and_branch_identity() {
    let first = included(&records(), &context()).unwrap();
    assert_eq!(first[0]["blockHeight"], u64::MAX.to_string());
    assert_eq!(first[0]["transactionGasUsed"], u64::MAX.to_string());
    assert_eq!(first[0]["origin"]["kind"], "transaction");
    assert_eq!(first[1]["origin"]["kind"], "child");
    assert_eq!(first[1]["creationIndex"], 1);
    assert_eq!(first[1]["verificationState"], "unverified");
    let mut next = context();
    next.block_hash = word(7);
    assert_ne!(
        first[0]["deploymentId"],
        included(&records(), &next).unwrap()[0]["deploymentId"]
    );
}
#[test]
fn malformed_or_wrong_chain_journals_fail_as_a_whole() {
    let mut wrong_chain = context();
    wrong_chain.chain_id = 9005;
    assert!(included(&records(), &wrong_chain).is_err());
    let mut malformed = records();
    malformed[1].code_hash = word(8);
    assert!(included(&malformed, &context()).is_err());
    let mut duplicate = records();
    duplicate.push(duplicate[0].clone());
    assert!(included(&duplicate, &context()).is_err());
    let mut no_block = context();
    no_block.block_hash = [0; 32];
    assert!(included(&records(), &no_block).is_err());
    assert!(included(&vec![records()[0].clone(); 65], &context()).is_err());
}
