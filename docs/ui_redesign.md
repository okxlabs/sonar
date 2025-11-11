# Solsim UI Redesign

## Design Goals
1. **Better Navigation**: Clear sections with visual hierarchy and easy scanning
2. **CPI Tree Structure**: Visual tree showing nested CPI calls with proper indentation
3. **Improved Information Architecture**: Group related information logically
4. **Enhanced Visual Design**: Better use of spacing, and symbols

## UI Structure

```
╔══════════════════════════════════════════════════════════════════════════════╗
║                           Solana Transaction Simulator                       ║
║                    (addresses are clickable links to Solscan)                ║
╚══════════════════════════════════════════════════════════════════════════════╝

📋 TRANSACTION SUMMARY
   Signature: zpcPSBcQpry4Mw8YyBz8nRCrbQ8Y1ZbvL7xN1WQtfYV4ohFnWgT9AGXXeKtp3AHUrTBBzaCiH9849Dbu7dMXoRh
   Status: 🟢 Success | Version: v0 | Encoding: base64
   Blockhash: 5hHGmm6CakjCnXyyTkLqxbkaaMxrTAqCwAtxE7xnVR83

   📊 35 accounts (2 signers, 8 programs, 25 writable) | 🔧 10 instructions | 📦 285,532 CUs

┌─ ADDRESS LOOKUP TABLES ──────────────────────────────────────────────────────┐
│                                                                              │
│  [0] 5ANPxYNcx2g8sPSZGWnWiry5mAnR25zC1GzmL4UwAetp (17 accounts)            │
│  [1] 9jmR4uKEfYFi1gjMJYEex25fn8NLTnSVy1TaLUoJ2TQU (12 accounts)            │
│  [2] Ga7MuV4c198RzhFvvpEHFVLUHaEDAM1VqW2rr2sJqfxe (6 accounts)             │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘

┌─ ACCOUNT REGISTRY ───────────────────────────────────────────────────────────┐
│                                                                              │
│  #0  DPLezAkFZ5sFaBXMWt3J2StQwYtcqecUipWSP7YfrLth 📜 🔑                     │
│  #1  3dZxNg54cFgm5AmtFbhVUudSj28KK5y3VSAaFDbeiZ9R 📜                        │
│  #2  4dSjQWcMFZ78a2H31iyaeyGB8b3MxSNFQwkUMt4MXCCP 📜                        │
│  #3  6HRgJMRmjaj2svP9GpRbUU5TPzLAHnBW3sHgYVbirWYE 📜                        │
│  #4  91fwzX1NZDgTNo1jBTd7rxPUhDA8i49Yo6B1ay1kZJ7s 📜                        │
│  #5  E14Pi7v4efxTmciDVMECeN6PKuu49gpJLcDbnwMEEvTB 📜                        │
│  #6  EGtPEX8kfh2DLPq2gLT8TvEg8HvwSS11uTC2b35ehWyD 📜                        │
│  #7  11111111111111111111111111111111            ⚙️                        │
│  #8  ComputeBudget111111111111111111111111111111   ⚙️                        │
│  #9  TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA ⚙️                        │
│  #10 jitonobundLe1111111111111111111111111111123   🔒                        │
│  #11 proVF4pMXVaYqmy4NjniPh4pqKNfMmsihgd4wdkCX3u ⚙️                        │
│  #12 8psNvWTrdNTiVRNzAgsou9kETXNJm2SXZyaKuJraVRtf 🔒                        │
│  #13 ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL ⚙️                        │
│  #14 LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo ⚙️                        │
│  #15 3JoExsYUAr3BZesyiap6H81aRszhLmST4XfaqTkMwrkt 📜                        │
│  #16 8gnggomXihtKZ7so3mXSmy5Ps4A4nUWt1URJppphhmtd 📜                        │
│  #17 8xZvtMYREsqJr1vVXLufQKefE2S7nsZECRGEiFirx6z9 📜                        │
│  #18 GQeRtTVS4A8dXXH3CJpr4R191vX6A1YZP2avPVCMmPBW 📜                        │
│  #19 GcB7qqgNWLnFfMydXxfrpyvWkAsHjpN8p2kZCQcJC7nR 📜                        │
│  #20 Ge73U4tNxgJezH1yC97wXTY4fkvwJtLGGPFACw1pQHdX 📜                        │
│  #21 cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG ⚙️                        │
│  #22 DvNW6mRWecUmAjuQEpBdnggRQfYHHQfwW1wCq6tVKbRT 📜                        │
│  #23 EoWfb1BKb7U2b9nsmKvdvSpQ1rcnqrhNtwgPA56TYsud 📜                        │
│  #24 Hi3PphuFfivUWQNecshRvVyCpWzw4FPTNvgN47oVjoHs 📜                        │
│  #25 ARu4n5mFdZogZAravu7CcizaojWnS6oqka37gdLT5SZn 📜                        │
│  #26 DT2krA8vSP96D68nH86eAztZHVM18YB5i4gDXjLBDXm7 📜                        │
│  #27 MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr ⚙️                        │
│  #28 So11111111111111111111111111111111111111112 🔒                        │
│  #29 D1ZN9Wj1fRSUQfCjhvnu1hqDMT7hzjzBBpi12nVniYD6 🔒                        │
│  #30 GaEiUW2i3dNtTa3Jq2CqXtHiTWXB7i6a6nd7uekpsnoW 🔒                        │
│  #31 3rmHSu74h1ZcmAisVcWerTCiRDQbUrBKmcwptYGjHfet 🔒                        │
│  #32 HLnpSz9h2S4hiLQ43rnSD9XkcUThA7B8hQMKmDaiTLcC 🔒                        │
│  #33 Sysvar1nstructions1111111111111111111111111 🔒                        │
│  #34 Ag3hiK9svNixH9Vu5sD2CmK5fyDWrx9a1iVSbZW22bUS 🔒                        │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘

┌─ INSTRUCTION TRACE ──────────────────────────────────────────────────────────┐
│                                                                              │
│  [0] ComputeBudget111111111111111111111111111111                             │
│      ⚓ [10] jitonobundLe1111111111111111111111111111123 🔒               │
│      🔢 0x02602b0b00 (5 bytes)                                            │
│                                                                              │
│  [1] ComputeBudget111111111111111111111111111111                             │
│      🔢 0x033c90010000000000 (9 bytes)                                    │
│                                                                              │
│  [2] 11111111111111111111111111111111                                       │
│      ⚓ [0] DPLezAkFZ5s...th 📜 🔑                                        │
│      ⚓ [2] 4dSjQWcMFZ78a... 📜                                           │
│      🔢 0x0200000070a23d0000000000 (12 bytes)                            │
│                                                                          │
│  [3] proVF4pMXVaYqmy4NjniPh4pqKNfMmsihgd4wdkCX3u                         │
│      ⚓ [0] DPLezAkFZ5sFaBXMWt3J2StQwYtcqecUipWSP7YfrLth 📜 🔑          │
│      ⚓ [0] DPLezAkFZ5sFaBXMWt3J2StQwYtcqecUipWSP7YfrLth 📜 🔑          │
│      ⚓ [2] 4dSjQWcMFZ78a2H31iyaeyGB8b3MxSNFQwkUMt4MXCCP 📜             │
│      🔍 [28] So11111111111111111111111111111111111111112 🔒             │
│      ⚓ [9] TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA ⚙️              │
│      ⚓ [7] 11111111111111111111111111111111 ⚙️                          │
│      🔢 0x93f17b64f484ae76fe (9 bytes)                                   │
│                                                                              │
│  [3.1] 11111111111111111111111111111111                                     │
│        ⚓ [0] DPLezAkFZ5sFaBXMWt3J2StQwYtcqecUipWSP7YfrLth 📜 🔑        │
│        ⚓ [2] 4dSjQWcMFZ78a2H31iyaeyGB8b3MxSNFQwkUMt4MXCCP 📜           │
│        🔢 0x0200000070a23d0000000000 (12 bytes)                          │
│                                                                              │
│  [3.2] TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA                         │
│        ⚓ [2] 4dSjQWcMFZ78a2H31iyaeyGB8b3MxSNFQwkUMt4MXCCP 📜           │
│        ⚓ [0] DPLezAkFZ5sFaBXMWt3J2StQwYtcqecUipWSP7YfrLth 📜 🔑        │
│        🔢 0x09 (1 byte)                                                  │
│                                                                          │
│  [4] ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL                        │
│      ⚓ [0] DPLezAkFZ5sFaBXMWt3J2StQwYtcqecUipWSP7YfrLth 📜 🔑          │
│      ⚓ [4] 91fwzX1NZDgTNo1jBTd7rxPUhDA8i49Yo6B1ay1kZJ7s 📜             │
│      ⚓ [0] DPLezAkFZ5sFaBXMWt3J2StQwYtcqecUipWSP7YfrLth 📜 🔑          │
│      🔍 [30] GaEiUW2i3dNtTa3Jq2CqXtHiTWXB7i6a6nd7uekpsnoW 🔒            │
│      ⚓ [7] 11111111111111111111111111111111 ⚙️                          │
│      ⚓ [9] TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA ⚙️              │
│      🔢 0x01 (1 byte)                                                    │
│                                                                          │
│  [5] ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL                        │
│      ⚓ [0] DPLezAkFZ5sFaBXMWt3J2StQwYtcqecUipWSP7YfrLth 📜 🔑          │
│      ⚓ [1] 3dZxNg54cFgm5AmtFbhVUudSj28KK5y3VSAaFDbeiZ9R 📜             │
│      ⚓ [12] 8psNvWTrdNTiVRNzAgsou9kETXNJm2SXZyaKuJraVRtf 🔒             │
│      🔍 [30] GaEiUW2i3dNtTa3Jq2CqXtHiTWXB7i6a6nd7uekpsnoW 🔒            │
│      ⚓ [7] 11111111111111111111111111111111 ⚙️                          │
│      ⚓ [9] TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA ⚙️              │
│      🔢 0x01 (1 byte)                                                    │
│                                                                              │
│  ... (4 more top-level instructions)                                         │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘

┌─ EXECUTION TRACE (CPI Tree) ─────────────────────────────────────────────────┐
│                                                                              │
│  🟢 Success | 285,532 CUs | 116 log messages                                │
│                                                                              │
│  [0] ComputeBudget111111111111111111111111111111                            │
│      invoke [1] → success                                                    │
│                                                                              │
│  [1] ComputeBudget111111111111111111111111111111                            │
│      invoke [1] → success                                                    │
│                                                                              │
│  [2] 11111111111111111111111111111111                                       │
│      invoke [1] → success                                                    │
│                                                                              │
│  [3] proVF4pMXVaYqmy4NjniPh4pqKNfMmsihgd4wdkCX3u (Marginfi)                 │
│      invoke [1] → success                                                    │
│      Program log: Instruction: CreateTokenAccount                           │
│                                                                              │
│  [3.1] 11111111111111111111111111111111                                     │
│        invoke [2] → success                                                  │
│                                                                              │
│  [3.2] TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA                         │
│        invoke [2] → success                                                  │
│        Program log: Instruction: InitializeAccount3                         │
│                                                                              │
│  [4] ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL                           │
│      invoke [1] → success                                                    │
│      Program log: CreateIdempotent                                          │
│                                                                              │
│  [5] ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL                           │
│      invoke [1] → success                                                    │
│      Program log: CreateIdempotent                                          │
│                                                                              │
│  [6] ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL                           │
│      invoke [1] → success                                                    │
│      Program log: CreateIdempotent                                          │
│                                                                              │
│  [7] proVF4pMXVaYqmy4NjniPh4pqKNfMmsihgd4wdkCX3u (Marginfi)                 │
│      invoke [1] → success                                                    │
│      Program log: Instruction: SwapTob                                      │
│                                                                              │
│  ... (remaining nested CPI calls collapsed)                                 │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘

┌─ SIMULATION SUMMARY ─────────────────────────────────────────────────────────┐
│                                                                              │
│  Return Data: None                                                           │
│  Accounts Modified: 12                                                       │
│  Program Replacements: 0                                                     │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘

┌─ LEGEND ─────────────────────────────────────────────────────────────────────┐
│                                                                              │
│  📜 Writable  🔒 Readonly  ⚙️ Program  🔑 Signer  ⚓ Static  🔍 Lookup       │
│  🔢 Data  [N] Top-level  [N.M] Nested CPI  CUs: Compute Units               │
│  All addresses shown in full (click for Solscan explorer)                   │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Key Improvements

### 1. Visual Hierarchy
- **Clear section headers** with box drawing characters (┌─┐└─┘)
- **status indicators** (🟢 Success, 🔴 Failure)
- **Consistent indentation** for nested structures
- **Logical grouping** of related information

### 2. Hierarchical Numbering System
- **Caller-callee relationship**: `[3]` calls `[3.1]`, `[3.2]`, etc.
- **Natural execution order**: Instructions listed in execution sequence
- **Unambiguous referencing**: "Instruction 3.1" is clear and specific
- **Tree representation**: Numbering itself shows the call hierarchy

### 3. CPI Tree Visualization
- **Hierarchical numbering** [1], [1.1], [1.1.1] for deep nesting
- **Indentation** visually represents call depth
- **Self-documenting structure** - numbering makes relationships obvious
- **Execution flow arrows** (→) show status transitions

### 4. Enhanced Navigation
- **Summary section at top** for quick overview
- **Account registry** with compact indexing
- **Instruction trace** with hierarchical numbering
- **Execution trace** as primary CPI tree view
- **Legend** for quick symbol reference

### 5. Information Density & Clarity
- **Emojis as primary indicators** (📜 🔒 ⚙️ 🔑)
- **Full addresses displayed** - no truncation for complete information
- **Data size indicators** (e.g., "5 bytes")
- **Consistent alignment** for easy scanning
- **Clickable links** - all addresses link to Solscan explorer

## Symbol Legend

- 📜 Writable account
- 🔒 Readonly account
- ⚙️ Program account
- 🔑 Signer
- 🔍 Address lookup table reference
- ⚓ Static account reference
- 🔢 Instruction data
- 🟢 Success
- 🔴 Failure
- 📋 Summary
- 📊 Statistics
- 📦 Compute units
- 🔧 Instructions

This design provides much better navigation, clearer visual hierarchy, and proper tree structure for CPI calls while maintaining all the important information from the original UI.

## Design Philosophy

### Minimalism & Clarity
- **Emojis as primary indicators**: 📜 🔒 ⚙️ 🔑 are self-explanatory through convention
- **Compact layout**: Tighter spacing while maintaining readability
- **Consistent alignment**: All elements line up for easy scanning

### Information Hierarchy
1. **Transaction Summary** - Most important info first
2. **Address Lookup Tables** - Context for the transaction
3. **Account Registry** - Reference data
4. **Instruction Trace** - What the transaction does
5. **Execution Trace** - How it executed (CPI tree)
6. **Simulation Summary** - Final results
7. **Legend** - Quick reference (collapsed by default in real UI)

### Visual Design Principles
- **Box drawing characters** (┌─┐└─┘) create clear sections without heavy ASCII art
- **Consistent indentation** (2 spaces) for tree structure
- **Full address display** - Complete addresses for maximum information
- **Alignment** - All instruction indices, account numbers, and arrows line up
- **Clickable links** - All addresses are hyperlinks to Solscan explorer

### Hierarchical Numbering System
The instruction trace now uses hierarchical numbering to clearly show caller-callee relationships:

```
[3] TopLevelInstruction          ← Top-level instruction
    ...

[3.1] NestedCPIInstruction      ← First CPI call from [3]
      ...

[3.2] AnotherNestedInstruction  ← Second CPI call from [3]
      ...
```

**Benefits:**
- **Clear parentage**: `[3.1]` and `[3.2]` are both called by `[3]`
- **Natural ordering**: Instructions execute in the order they appear
- **Easy referencing**: "Instruction 3.1" is unambiguous
- **Tree representation**: The numbering itself shows the call hierarchy

### CPI Tree Structure
The execution trace shows the nested nature of CPI calls:
```
[3] proVF4pMXVaYqmy4NjniPh4pqKNfMmsihgd4wdkCX3u
    invoke [1] → success

    [3.1] 11111111111111111111111111111111
          invoke [2] → success

    [3.2] TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
          invoke [2] → success
```

Key features:
- **Hierarchical numbering** [3], [3.1], [3.1.1], [3.1.2], [3.2] shows caller-callee relationships
- **Indentation** visually represents the call tree
- **Arrows** (→) show execution flow
- **CUs** shown at each level for performance analysis
- **Self-documenting** - numbering makes relationships obvious
