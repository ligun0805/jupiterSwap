import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { Swap } from "../target/types/swap";
import { TOKEN_PROGRAM_ID, createMint, getOrCreateAssociatedTokenAccount } from "@solana/spl-token";

// Constants
const JUPITER_PROGRAM_ID = new anchor.web3.PublicKey("JUP6i4ozu5ydDCnLiMogSckDPpbtr7BJ4FtzYWkb5Rk");
const USDC_MINT = new anchor.web3.PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

async function main() {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  if (!provider.wallet.payer) {
    throw new Error("Wallet payer is not available");
  }

  // Fork Jupiter program
  console.log("Forking Jupiter program...");
  const jupiterProgram = await anchor.Program.at(
    JUPITER_PROGRAM_ID,
    provider
  );
  console.log("Jupiter program forked successfully!");

  // Fork USDC mint
  console.log("Forking USDC mint...");
  const usdcMint = await createMint(
    provider.connection,
    provider.wallet.payer,
    provider.wallet.publicKey,
    null,
    6 // USDC has 6 decimals
  );
  console.log("USDC mint forked successfully! Mint address:", usdcMint.toBase58());

  // Create USDC token account for testing
  console.log("Creating USDC token account...");
  const usdcTokenAccount = await getOrCreateAssociatedTokenAccount(
    provider.connection,
    provider.wallet.payer,
    usdcMint,
    provider.wallet.publicKey
  );
  console.log("USDC token account created successfully! Account address:", usdcTokenAccount.address.toBase58());

  // Update Anchor.toml with forked addresses
  console.log("Updating Anchor.toml with forked addresses...");
  const anchorToml = require("../Anchor.toml");
  anchorToml.programs.localnet.jupiter = JUPITER_PROGRAM_ID.toBase58();
  anchorToml.programs.localnet.usdc = usdcMint.toBase58();
  require("fs").writeFileSync("../Anchor.toml", JSON.stringify(anchorToml, null, 2));
  console.log("Anchor.toml updated successfully!");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
}); 