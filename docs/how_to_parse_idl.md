# how to parse anchor IDL files in Rust

## Goal
This document provides a minimal example of an Anchor IDL (Interface Definition Language) file and explains its structure for parsing in Rust.

## Minimum example
```json
{
  "address": "BYFW1vhC1ohxwRbYoLbAWs86STa25i9sD5uEusVjTYNd",
  "metadata": {
    "name": "hello_anchor",
    "version": "0.1.0",
    "spec": "0.1.0",
    "description": "Created with Anchor"
  },
  "instructions": [
    {
      "name": "initialize",
      "discriminator": [175, 175, 109, 31, 13, 152, 155, 237],
      "accounts": [
        {
          "name": "new_account",
          "writable": true,
          "signer": true
        },
        {
          "name": "signer",
          "writable": true,
          "signer": true
        },
        {
          "name": "system_program",
          "address": "11111111111111111111111111111111"
        }
      ],
      "args": [
        {
          "name": "data",
          "type": "u64"
        }
      ]
    }
  ],
  "accounts": [
    {
      "name": "NewAccount",
      "discriminator": [176, 95, 4, 118, 91, 177, 125, 232]
    }
  ],
  "types": [
    {
      "name": "NewAccount",
      "type": {
        "kind": "struct",
        "fields": [
          {
            "name": "data",
            "type": "u64"
          }
        ]
      }
    }
  ]
}
```
### Structure Explanation
- `address`: The program's public key on the Solana blockchain.
- `metadata`: Contains metadata about the program, including its name, version, and description. (we don't care about this for parsing)
- `instructions`: An array of instructions that the program can execute. Each instruction includes:
  - `name`: The name of the instruction.
  - `discriminator`: A unique byte array that identifies the instruction.
  - `accounts`: The accounts required for the instruction, specifying whether they are writable or signers.
  - `args`: The arguments that the instruction takes, including their names and types.
- `accounts`: An array defining the account structures used by the program, including their names and discriminators.
- `types`: Custom data types defined by the program, including their names and fields.
  - `name`: The name of the custom type.
  - `type`: The structure of the type, including its kind (e.g., struct, enum) and fields with their names and types.

### troubleshooting
- when you don't know how to parse a type in the IDL, you should refer to the top level "types" array to find the definition of that type.
- for example, if you see a type "NewAccount" in the instruction args, you can look it up in the "types" array to find its structure.
- primitive types like "u64", "u8", "bool", etc. can be directly mapped to Rust types.
- complex types like "struct" or "enum" need to be parsed according to their definitions in the "types" array.
- instruction args are serialized use Borsh, so you should parse them as per the Borsh sepcification.
