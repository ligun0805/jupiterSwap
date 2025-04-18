import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Swap } from "../target/types/swap";
import { TOKEN_PROGRAM_ID } from "@solana/spl-token";

// Set mainnet RPC URL
process.env.ANCHOR_PROVIDER_URL = "https://api.devnet.solana.com";

// Constants
const JUPITER_PROGRAM_ID = new anchor.web3.PublicKey("JUP6i4ozu5ydDCnLiMogSckDPpbtr7BJ4FtzYWkb5Rk");
const USDC_MINT = new anchor.web3.PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

async function main() {
  // Configure the client to use the mainnet cluster
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  if (!provider.wallet.payer) {
    throw new Error("Wallet payer is not available");
  }

  // Load the program
  const program = anchor.workspace.Swap as Program<Swap>;

  console.log("Deploying program to mainnet...");
  console.log("Program ID:", program.programId.toBase58());
  console.log("Deployer wallet:", provider.wallet.publicKey.toBase58());

  // Generate a new keypair for the swap account
  const swapAccount = anchor.web3.Keypair.generate();

  // Deploy the program
  try {
    const deployTx = await program.methods
      .initialize(
        provider.wallet.publicKey, // admin
        provider.wallet.publicKey  // referral (using same wallet for testing)
      )
      .accounts({
        swapAccount: {
          pubkey: swapAccount.publicKey,
          isWritable: true,
          isSigner: false,
        },
        admin: {
          pubkey: provider.wallet.publicKey,
          isWritable: true,
          isSigner: true,
        },
        referral: {
          pubkey: provider.wallet.publicKey,
          isWritable: true,
          isSigner: true,
        },
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([swapAccount])
      .rpc();

    console.log("Deployment successful!");
    console.log("Transaction signature:", deployTx);
    console.log("Program deployed to:", program.programId.toBase58());
    console.log("Swap account:", swapAccount.publicKey.toBase58());

    // Verify the deployment
    const programInfo = await provider.connection.getAccountInfo(program.programId);
    if (programInfo) {
      console.log("Program verified successfully!");
      console.log("Program data length:", programInfo.data.length);
    } else {
      console.error("Failed to verify program deployment");
    }

  } catch (error) {
    console.error("Deployment failed:", error);
    process.exit(1);
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
}); 