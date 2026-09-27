/**
 * Integration test scaffold for the happy path: initialize_config ->
 * initialize_herd (PayeeMode.Me) -> deposit_fee_split -> harvest_burn,
 * asserting the pro-rata payout math.
 *
 * Requires `anchor test` (a local validator + `anchor build`'s generated
 * IDL/types at target/types/zebra) — not runnable by plain `ts-mocha`
 * outside that harness, and not run as part of this repo's `cargo check`.
 * $ZEBRA / stampede and Holders-mode coverage are left as TODOs: they need
 * a real (or mocked) AMM program to swap against, which is out of scope
 * for a first pass at this scaffold — see
 * programs/zebra/src/instructions/execute_stampede_swap_and_burn.rs's
 * header comment for why that CPI is generic/pluggable rather than
 * hardcoded against one AMM.
 */

import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import {
  createAssociatedTokenAccountIdempotent,
  createMint,
  getAssociatedTokenAddressSync,
  mintTo,
} from "@solana/spl-token";
import { Keypair, PublicKey, SystemProgram } from "@solana/web3.js";
import { assert } from "chai";

// `anchor build` generates this; not present until that's been run once.
// eslint-disable-next-line @typescript-eslint/no-var-requires
type ZebraProgram = Program<any>;

describe("zebra", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.Zebra as ZebraProgram;
  const connection = provider.connection;
  const payer = (provider.wallet as anchor.Wallet).payer;

  const admin = payer;
  const keeper = Keypair.generate();
  const protocolTreasury = Keypair.generate().publicKey;
  const recoveryAddress = Keypair.generate().publicKey;
  const fakeSwapProgram = Keypair.generate().publicKey; // never actually invoked in this test

  let wzecMint: PublicKey;
  let zebraMint: PublicKey;
  let coinMint: PublicKey;
  const coinMintAuthority = payer; // simplification: payer is the coin's own mint authority

  const configPda = PublicKey.findProgramAddressSync(
    [Buffer.from("config")],
    program.programId,
  )[0];
  const feeSplitPda = PublicKey.findProgramAddressSync(
    [Buffer.from("fee_split")],
    program.programId,
  )[0];
  const stampedeAuthorityPda = PublicKey.findProgramAddressSync(
    [Buffer.from("stampede")],
    program.programId,
  )[0];

  before(async () => {
    wzecMint = await createMint(connection, payer, payer.publicKey, null, 8);
    zebraMint = await createMint(connection, payer, payer.publicKey, null, 9);
    coinMint = await createMint(connection, payer, coinMintAuthority.publicKey, null, 6);
  });

  it("initializes global config + fee split + stampede vault", async () => {
    await program.methods
      .initializeConfig(
        admin.publicKey,
        keeper.publicKey,
        protocolTreasury,
        zebraMint,
        fakeSwapProgram,
        recoveryAddress,
        null,
        null,
      )
      .accounts({
        payer: payer.publicKey,
        globalConfig: configPda,
        feeSplit: feeSplitPda,
        wzecMint,
        stampedeAuthority: stampedeAuthorityPda,
        stampedeVault: getAssociatedTokenAddressSync(wzecMint, stampedeAuthorityPda, true),
        tokenProgram: anchor.utils.token.TOKEN_PROGRAM_ID,
        associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    const cfg = await program.account.globalConfig.fetch(configPda);
    assert.equal(cfg.wzecMint.toBase58(), wzecMint.toBase58());

    const feeSplit = await program.account.feeSplitConfig.fetch(feeSplitPda);
    assert.equal(feeSplit.herdBps + feeSplit.stampedeBps + feeSplit.treasuryBps + feeSplit.deployerBps, 200);
  });

  let herdPda: PublicKey;
  let herdZecVault: PublicKey;
  let deployerAuthorityPda: PublicKey;
  let deployerVault: PublicKey;

  it("plants a herd for coinMint in PayeeMode.Me", async () => {
    herdPda = PublicKey.findProgramAddressSync(
      [Buffer.from("herd"), coinMint.toBuffer()],
      program.programId,
    )[0];
    deployerAuthorityPda = PublicKey.findProgramAddressSync(
      [Buffer.from("herd_deployer"), coinMint.toBuffer()],
      program.programId,
    )[0];
    herdZecVault = getAssociatedTokenAddressSync(wzecMint, herdPda, true);
    deployerVault = getAssociatedTokenAddressSync(wzecMint, deployerAuthorityPda, true);

    await program.methods
      .initializeHerd({ me: {} }, null, new anchor.BN(1_000_000_000))
      .accounts({
        payer: payer.publicKey,
        mintAuthority: coinMintAuthority.publicKey,
        coinMint,
        globalConfig: configPda,
        herd: herdPda,
        wzecMint,
        zecVault: herdZecVault,
        deployerAuthority: deployerAuthorityPda,
        deployerVault,
        tokenProgram: anchor.utils.token.TOKEN_PROGRAM_ID,
        associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    const herd = await program.account.herd.fetch(herdPda);
    assert.equal(herd.coinMint.toBase58(), coinMint.toBase58());
    assert.equal(herd.totalZecDeposited.toNumber(), 0);
  });

  it("splits a deposited fee into the four buckets", async () => {
    const feeAuthority = payer;
    const feeSource = await createAssociatedTokenAccountIdempotent(
      connection,
      payer,
      wzecMint,
      feeAuthority.publicKey,
    );
    await mintTo(connection, payer, wzecMint, feeSource, payer, 1_000_000); // 0.01 wZEC @ 8 decimals

    const treasuryAta = getAssociatedTokenAddressSync(wzecMint, protocolTreasury);
    const stampedeVault = getAssociatedTokenAddressSync(wzecMint, stampedeAuthorityPda, true);

    await program.methods
      .depositFeeSplit(new anchor.BN(1_000_000))
      .accounts({
        payer: payer.publicKey,
        globalConfig: configPda,
        feeSplit: feeSplitPda,
        coinMint,
        herd: herdPda,
        feeAuthority: feeAuthority.publicKey,
        feeSource,
        herdZecVault,
        deployerVault,
        stampedeVault,
        protocolTreasury,
        treasuryZecAta: treasuryAta,
        wzecMint,
        tokenProgram: anchor.utils.token.TOKEN_PROGRAM_ID,
        associatedTokenProgram: anchor.utils.token.ASSOCIATED_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    const herd = await program.account.herd.fetch(herdPda);
    // herd_bps=50/200 of 1_000_000 => 250_000
    assert.equal(herd.totalZecDeposited.toNumber(), 250_000);
  });

  it("burns coin supply and pays out pro-rata from the herd vault", async () => {
    const burner = payer;
    const burnerCoinAccount = await createAssociatedTokenAccountIdempotent(
      connection,
      payer,
      coinMint,
      burner.publicKey,
    );
    await mintTo(connection, payer, coinMint, burnerCoinAccount, coinMintAuthority, 1_000_000);

    const burnerZecAta = await createAssociatedTokenAccountIdempotent(
      connection,
      payer,
      wzecMint,
      burner.publicKey,
    );

    await program.methods
      .harvestBurn(new anchor.BN(100_000), null)
      .accounts({
        burner: burner.publicKey,
        globalConfig: configPda,
        coinMint,
        herd: herdPda,
        herdZecVault,
        burnerCoinAccount,
        burnerZecAta,
        tokenProgram: anchor.utils.token.TOKEN_PROGRAM_ID,
      })
      .rpc();

    // payout = total_zec_deposited * burn_amount / supply_before_burn
    //        = 250_000 * 100_000 / 1_000_000 = 25_000
    const zecAccount = await connection.getTokenAccountBalance(burnerZecAta);
    assert.equal(zecAccount.value.amount, "25000");
  });
});
