use std::collections::HashSet;

use rand_core::OsRng;

use monero_simple_request_rpc::SimpleRequestRpc;
use monero_wallet::{
  ringct::RctType, transaction::Transaction, rpc::Rpc, address::SubaddressIndex, extra::Extra,
  WalletOutput, OutputWithDecoys,
};

mod runner;
use runner::{SignableTransactionBuilder, ring_len};

// Set up inputs, select decoys, then add them to the TX builder
async fn add_inputs(
  rct_type: RctType,
  rpc: &SimpleRequestRpc,
  outputs: Vec<WalletOutput>,
  builder: &mut SignableTransactionBuilder,
) {
  for output in outputs {
    builder.add_input(
      OutputWithDecoys::fingerprintable_deterministic_new(
        &mut OsRng,
        rpc,
        ring_len(rct_type),
        rpc.get_height().await.unwrap(),
        output,
      )
      .await
      .unwrap(),
    );
  }
}

test!(
  spend_miner_output,
  (
    |_, mut builder: Builder, addr| async move {
      builder.add_payment(addr, 5);
      (builder.build().unwrap(), ())
    },
    |rpc, block, tx: Transaction, mut scanner: Scanner, ()| async move {
      let output =
        scanner.scan(&rpc, &block).await.unwrap().not_additionally_locked().swap_remove(0);
      assert_eq!(output.transaction(), tx.hash());
      assert_eq!(output.commitment().amount, 5);
    },
  ),
);

test!(
  spend_multiple_outputs,
  (
    |_, mut builder: Builder, addr| async move {
      builder.add_payment(addr, 1000000000000);
      builder.add_payment(addr, 2000000000000);
      (builder.build().unwrap(), ())
    },
    |rpc, block, tx: Transaction, mut scanner: Scanner, ()| async move {
      let mut outputs = scanner.scan(&rpc, &block).await.unwrap().not_additionally_locked();
      assert_eq!(outputs.len(), 2);
      assert_eq!(outputs[0].transaction(), tx.hash());
      assert_eq!(outputs[0].transaction(), tx.hash());
      outputs.sort_by(|x, y| x.commitment().amount.cmp(&y.commitment().amount));
      assert_eq!(outputs[0].commitment().amount, 1000000000000);
      assert_eq!(outputs[1].commitment().amount, 2000000000000);
      outputs
    },
  ),
  (
    |rct_type: RctType, rpc, mut builder: Builder, addr, outputs: Vec<WalletOutput>| async move {
      add_inputs(rct_type, &rpc, outputs, &mut builder).await;
      builder.add_payment(addr, 6);
      (builder.build().unwrap(), ())
    },
    |rpc, block, tx: Transaction, mut scanner: Scanner, ()| async move {
      let output =
        scanner.scan(&rpc, &block).await.unwrap().not_additionally_locked().swap_remove(0);
      assert_eq!(output.transaction(), tx.hash());
      assert_eq!(output.commitment().amount, 6);
    },
  ),
);

test!(
  // Ideally, this would be single_R, yet it isn't feasible to apply allow(non_snake_case) here
  single_r_subaddress_send,
  (
    // Consume this builder for an output we can use in the future
    // This is needed because we can't get the input from the passed in builder
    |_, mut builder: Builder, addr| async move {
      builder.add_payment(addr, 1000000000000);
      (builder.build().unwrap(), ())
    },
    |rpc, block, tx: Transaction, mut scanner: Scanner, ()| async move {
      let outputs = scanner.scan(&rpc, &block).await.unwrap().not_additionally_locked();
      assert_eq!(outputs.len(), 1);
      assert_eq!(outputs[0].transaction(), tx.hash());
      assert_eq!(outputs[0].commitment().amount, 1000000000000);
      outputs
    },
  ),
  (
    |rct_type, rpc: SimpleRequestRpc, _, _, outputs: Vec<WalletOutput>| async move {
      use monero_wallet::rpc::FeePriority;

      let view_priv = Zeroizing::new(Scalar::random(&mut OsRng));
      let mut outgoing_view = Zeroizing::new([0; 32]);
      OsRng.fill_bytes(outgoing_view.as_mut());
      let change_view =
        ViewPair::new(&Scalar::random(&mut OsRng) * ED25519_BASEPOINT_TABLE, view_priv.clone())
          .unwrap();

      let mut builder = SignableTransactionBuilder::new(
        rct_type,
        outgoing_view,
        Change::new(change_view.clone(), None),
        rpc.get_fee_rate(FeePriority::Unimportant).await.unwrap(),
      );
      add_inputs(rct_type, &rpc, vec![outputs.first().unwrap().clone()], &mut builder).await;

      // Send to a subaddress
      let sub_view = ViewPair::new(
        &Scalar::random(&mut OsRng) * ED25519_BASEPOINT_TABLE,
        Zeroizing::new(Scalar::random(&mut OsRng)),
      )
      .unwrap();
      builder
        .add_payment(sub_view.subaddress(Network::Mainnet, SubaddressIndex::new(0, 1).unwrap()), 1);
      (builder.build().unwrap(), (change_view, sub_view))
    },
    |rpc, block, tx: Transaction, _, views: (ViewPair, ViewPair)| async move {
      // Make sure the change can pick up its output
      let mut change_scanner = Scanner::new(views.0);
      assert!(
        change_scanner.scan(&rpc, &block).await.unwrap().not_additionally_locked().len() == 1
      );

      // Make sure the subaddress can pick up its output
      let mut sub_scanner = Scanner::new(views.1);
      sub_scanner.register_subaddress(SubaddressIndex::new(0, 1).unwrap());
      let sub_outputs = sub_scanner.scan(&rpc, &block).await.unwrap().not_additionally_locked();
      assert!(sub_outputs.len() == 1);
      assert_eq!(sub_outputs[0].transaction(), tx.hash());
      assert_eq!(sub_outputs[0].commitment().amount, 1);
      assert!(sub_outputs[0].subaddress().unwrap().account() == 0);
      assert!(sub_outputs[0].subaddress().unwrap().address() == 1);

      // Make sure only one R was included in TX extra
      assert!(Extra::read::<&[u8]>(&mut tx.prefix().extra.as_ref())
        .unwrap()
        .keys()
        .unwrap()
        .1
        .is_none());
    },
  ),
);

test!(
  spend_one_input_to_one_output_plus_change,
  (
    |_, mut builder: Builder, addr| async move {
      builder.add_payment(addr, 2000000000000);
      (builder.build().unwrap(), ())
    },
    |rpc, block, tx: Transaction, mut scanner: Scanner, ()| async move {
      let outputs = scanner.scan(&rpc, &block).await.unwrap().not_additionally_locked();
      assert_eq!(outputs.len(), 1);
      assert_eq!(outputs[0].transaction(), tx.hash());
      assert_eq!(outputs[0].commitment().amount, 2000000000000);
      outputs
    },
  ),
  (
    |rct_type: RctType, rpc, mut builder: Builder, addr, outputs: Vec<WalletOutput>| async move {
      add_inputs(rct_type, &rpc, outputs, &mut builder).await;
      builder.add_payment(addr, 2);
      (builder.build().unwrap(), ())
    },
    |rpc, block, tx: Transaction, mut scanner: Scanner, ()| async move {
      let output =
        scanner.scan(&rpc, &block).await.unwrap().not_additionally_locked().swap_remove(0);
      assert_eq!(output.transaction(), tx.hash());
      assert_eq!(output.commitment().amount, 2);
    },
  ),
);

test!(
  spend_max_outputs,
  (
    |_, mut builder: Builder, addr| async move {
      builder.add_payment(addr, 1000000000000);
      (builder.build().unwrap(), ())
    },
    |rpc, block, tx: Transaction, mut scanner: Scanner, ()| async move {
      let outputs = scanner.scan(&rpc, &block).await.unwrap().not_additionally_locked();
      assert_eq!(outputs.len(), 1);
      assert_eq!(outputs[0].transaction(), tx.hash());
      assert_eq!(outputs[0].commitment().amount, 1000000000000);
      outputs
    },
  ),
  (
    |rct_type: RctType, rpc, mut builder: Builder, addr, outputs: Vec<WalletOutput>| async move {
      add_inputs(rct_type, &rpc, outputs, &mut builder).await;

      for i in 0 .. 15 {
        builder.add_payment(addr, i + 1);
      }
      (builder.build().unwrap(), ())
    },
    |rpc, block, tx: Transaction, mut scanner: Scanner, ()| async move {
      let mut scanned_tx = scanner.scan(&rpc, &block).await.unwrap().not_additionally_locked();

      let mut output_amounts = HashSet::new();
      for i in 0 .. 15 {
        output_amounts.insert(i + 1);
      }
      for _ in 0 .. 15 {
        let output = scanned_tx.swap_remove(0);
        assert_eq!(output.transaction(), tx.hash());
        let amount = output.commitment().amount;
        assert!(output_amounts.remove(&amount));
      }
      assert_eq!(output_amounts.len(), 0);
    },
  ),
);

test!(
  spend_max_outputs_to_subaddresses,
  (
    |_, mut builder: Builder, addr| async move {
      builder.add_payment(addr, 1000000000000);
      (builder.build().unwrap(), ())
    },
    |rpc, block, tx: Transaction, mut scanner: Scanner, ()| async move {
      let outputs = scanner.scan(&rpc, &block).await.unwrap().not_additionally_locked();
      assert_eq!(outputs.len(), 1);
      assert_eq!(outputs[0].transaction(), tx.hash());
      assert_eq!(outputs[0].commitment().amount, 1000000000000);
      outputs
    },
  ),
  (
    |rct_type: RctType, rpc, mut builder: Builder, _, outputs: Vec<WalletOutput>| async move {
      add_inputs(rct_type, &rpc, outputs, &mut builder).await;

      let view = runner::random_address().1;
      let mut scanner = Scanner::new(view.clone());

      let mut subaddresses = vec![];
      for i in 0 .. 15 {
        let subaddress = SubaddressIndex::new(0, i + 1).unwrap();
        scanner.register_subaddress(subaddress);

        builder.add_payment(view.subaddress(Network::Mainnet, subaddress), u64::from(i + 1));
        subaddresses.push(subaddress);
      }

      (builder.build().unwrap(), (scanner, subaddresses))
    },
    |rpc, block, tx: Transaction, _, mut state: (Scanner, Vec<SubaddressIndex>)| async move {
      use std::collections::HashMap;

      let mut scanned_tx = state.0.scan(&rpc, &block).await.unwrap().not_additionally_locked();

      let mut output_amounts_by_subaddress = HashMap::new();
      for i in 0 .. 15 {
        output_amounts_by_subaddress.insert(u64::try_from(i + 1).unwrap(), state.1[i]);
      }
      for _ in 0 .. 15 {
        let output = scanned_tx.swap_remove(0);
        assert_eq!(output.transaction(), tx.hash());
        let amount = output.commitment().amount;

        assert_eq!(
          output.subaddress().unwrap(),
          output_amounts_by_subaddress.remove(&amount).unwrap()
        );
      }
      assert_eq!(output_amounts_by_subaddress.len(), 0);
    },
  ),
);

test!(
  spend_one_input_to_two_outputs_no_change,
  (
    |_, mut builder: Builder, addr| async move {
      builder.add_payment(addr, 1000000000000);
      (builder.build().unwrap(), ())
    },
    |rpc, block, tx: Transaction, mut scanner: Scanner, ()| async move {
      let outputs = scanner.scan(&rpc, &block).await.unwrap().not_additionally_locked();
      assert_eq!(outputs.len(), 1);
      assert_eq!(outputs[0].transaction(), tx.hash());
      assert_eq!(outputs[0].commitment().amount, 1000000000000);
      outputs
    },
  ),
  (
    |rct_type, rpc: SimpleRequestRpc, _, addr, outputs: Vec<WalletOutput>| async move {
      use monero_wallet::rpc::FeePriority;

      let mut outgoing_view = Zeroizing::new([0; 32]);
      OsRng.fill_bytes(outgoing_view.as_mut());
      let mut builder = SignableTransactionBuilder::new(
        rct_type,
        outgoing_view,
        Change::fingerprintable(None),
        rpc.get_fee_rate(FeePriority::Unimportant).await.unwrap(),
      );
      add_inputs(rct_type, &rpc, vec![outputs.first().unwrap().clone()], &mut builder).await;
      builder.add_payment(addr, 10000);
      builder.add_payment(addr, 50000);

      (builder.build().unwrap(), ())
    },
    |rpc, block, tx: Transaction, mut scanner: Scanner, ()| async move {
      let mut outputs = scanner.scan(&rpc, &block).await.unwrap().not_additionally_locked();
      assert_eq!(outputs.len(), 2);
      assert_eq!(outputs[0].transaction(), tx.hash());
      assert_eq!(outputs[1].transaction(), tx.hash());
      outputs.sort_by(|x, y| x.commitment().amount.cmp(&y.commitment().amount));
      assert_eq!(outputs[0].commitment().amount, 10000);
      assert_eq!(outputs[1].commitment().amount, 50000);

      // The remainder should get shunted to fee, which is fingerprintable
      let Transaction::V2 { proofs: Some(ref proofs), .. } = tx else { panic!("TX wasn't RingCT") };
      assert_eq!(proofs.base.fee, 1000000000000 - 10000 - 50000);
    },
  ),
);

test!(
  subaddress_change,
  (
    // Consume this builder for an output we can use in the future
    // This is needed because we can't get the input from the passed in builder
    |_, mut builder: Builder, addr| async move {
      builder.add_payment(addr, 1000000000000);
      (builder.build().unwrap(), ())
    },
    |rpc, block, tx: Transaction, mut scanner: Scanner, ()| async move {
      let outputs = scanner.scan(&rpc, &block).await.unwrap().not_additionally_locked();
      assert_eq!(outputs.len(), 1);
      assert_eq!(outputs[0].transaction(), tx.hash());
      assert_eq!(outputs[0].commitment().amount, 1000000000000);
      outputs
    },
  ),
  (
    |rct_type, rpc: SimpleRequestRpc, _, _, outputs: Vec<WalletOutput>| async move {
      use monero_wallet::rpc::FeePriority;

      let view_priv = Zeroizing::new(Scalar::random(&mut OsRng));
      let mut outgoing_view = Zeroizing::new([0; 32]);
      OsRng.fill_bytes(outgoing_view.as_mut());
      let change_view =
        ViewPair::new(&Scalar::random(&mut OsRng) * ED25519_BASEPOINT_TABLE, view_priv.clone())
          .unwrap();

      let mut builder = SignableTransactionBuilder::new(
        rct_type,
        outgoing_view,
        Change::new(change_view.clone(), Some(SubaddressIndex::new(0, 1).unwrap())),
        rpc.get_fee_rate(FeePriority::Unimportant).await.unwrap(),
      );
      add_inputs(rct_type, &rpc, vec![outputs.first().unwrap().clone()], &mut builder).await;

      // Send to a random address
      let view = ViewPair::new(
        &Scalar::random(&mut OsRng) * ED25519_BASEPOINT_TABLE,
        Zeroizing::new(Scalar::random(&mut OsRng)),
      )
      .unwrap();
      builder.add_payment(view.legacy_address(Network::Mainnet), 1);
      (builder.build().unwrap(), change_view)
    },
    |rpc, block, _, _, change_view: ViewPair| async move {
      // Make sure the change can pick up its output
      let mut change_scanner = Scanner::new(change_view);
      change_scanner.register_subaddress(SubaddressIndex::new(0, 1).unwrap());
      let outputs = change_scanner.scan(&rpc, &block).await.unwrap().not_additionally_locked();
      assert!(outputs.len() == 1);
      assert!(outputs[0].subaddress().unwrap().account() == 0);
      assert!(outputs[0].subaddress().unwrap().address() == 1);
    },
  ),
);
