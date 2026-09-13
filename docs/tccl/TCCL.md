# TCCL — The Coin Cloud Language

TCCL is the smart-contract language of The Coin. It is small, indentation-based (like Python) and
**strict**: every variable has a declared type, there are no implicit conversions, no floats, no
null, every operation is metered with fuel, and the compiler is part of the consensus rules — a
deployment transaction carries the source code and every node compiles it identically.

> **The complete guide is the [TCCL Cookbook (PDF)](tccl-cookbook.pdf)**: installation, a first
> contract in ten minutes, a full language tour, storage deposits, fuel and fees with real numbers,
> deploying to the network, nine tested recipes (tip jar, token, crowdfunding, escrow, poll, savings,
> treasury, name service, private payments pool), a security checklist and the complete reference.
> This page is the quick start and the language reference.

Language version 1 · `tccl` 0.2.0 · implementation: [`crates/tccl`](../../crates/tccl) ·
examples: [`crates/tccl/examples`](../../crates/tccl/examples)

**Contents:** [Quick start](#quick-start) · [Language](#language-reference) ·
[Built-ins](#built-in-functions) · [Storage and deposit](#storage-and-the-refundable-deposit) ·
[Fuel and fees](#fuel-and-fees) · [Limits](#limits) · [Tools](#tools) · [Errors](#runtime-errors) ·
[Security](#security-checklist) · [Examples](#example-contracts)

---

## Quick start

### 1. Install

The node installer also installs the `tccl` developer tool:

```sh
curl -fsSL https://the-coin.cloud/install.sh | sudo bash
```

Or build from source (any platform with Rust):

```sh
git clone https://github.com/LucasBolla94/thecoin.git && cd thecoin
cargo build --release -p tccl -p thecoin-wallet -p thecoin-node
```

### 2. Write a contract

`counter.tccl`:

```tccl
# The smallest useful contract: a counter anyone can increase.
contract Counter

state count: int
state last_caller: address

event Increased(by: address, amount: int, total: int)

action increment(amount: int):
    require amount > 0, "amount must be positive"
    require amount <= 100, "at most 100 per call"
    count += amount
    last_caller = caller
    emit Increased(caller, amount, count)

view get() -> int:
    return count

view last() -> address:
    return last_caller
```

### 3. Check and run it locally

`tccl check` compiles exactly like the network; `tccl run` executes the contract in a local
simulator (state kept in `tccl-state.json`; test accounts `alice`, `bob`, … start with 1 000 000 TCN):

```text
$ tccl check counter.tccl
✔ Counter compiles (484 bytes of source, 323 bytes compiled)
  state: count: int, last_caller: address
  action increment(amount: int)
  view get() -> int
  view last() -> address
$ tccl run counter.tccl deploy
deployed Counter at daa436158c1dcdc0242085b6dd9451e33496cbee
ok · fuel used: 2420 · height: 2 · alice balance: 100000000000000 motes
$ tccl run counter.tccl call increment 5
event Increased(by: tcr1zqygp3t5ugluq2uqf7gq60mc9z9t09lpkg42sa, amount: 5, total: 5)
ok · fuel used: 1674 · height: 3 · alice balance: 100000000000000 motes
$ tccl run counter.tccl --from bob call increment 7
event Increased(by: tcr14vqs008pe0yx022r6pneffl2mnn0x2lkzh7452, amount: 7, total: 12)
ok · fuel used: 1674 · height: 4 · bob balance: 100000000000000 motes
$ tccl run counter.tccl view get
result: 12
ok · fuel used: 273 · height: 4 · alice balance: 100000000000000 motes
$ tccl run counter.tccl view last
result: tcr14vqs008pe0yx022r6pneffl2mnn0x2lkzh7452
ok · fuel used: 273 · height: 4 · alice balance: 100000000000000 motes
$ tccl run counter.tccl call increment 500
FAILED: requirement failed: at most 100 per call · fuel used: 33 (all changes reverted)
```

### 4. Deploy and use it on the network

Commands are for mainnet; add `--network testnet` while learning. Real output recorded on a private
regtest network (addresses `tcr1…`):

```text
$ thecoin-wallet -y contract deploy counter.tccl
Fuel:     2420 measured, limit 8146
From:     tcr1a9edrkqqhylclwfdaf8sk78zgfkkc0ctvk4awq
Action:   deploy contract Counter (484 bytes of TCCL)
Fee:      0.00001948 TCN (643 bytes, priority Normal)
Broadcast OK. txid: 28e99a814f77e26f776274cef2e2e80db1703f15b360690b26d3aef0a03fdd60
Contract address: tcr17kp5hrqhpdkwmukkrsfp6xft6y9l8j02rhxmvz
$ thecoin-wallet -y contract invoke tcr17kp5hrqhpdkwmukkrsfp6xft6y9l8j02rhxmvz increment 5
Fuel:     1777 measured, limit 7310
Preview:  Increased(by: tcr1a9edrkqqhylclwfdaf8sk78zgfkkc0ctvk4awq, amount: 5, total: 5) (simulated on the current state)
From:     tcr1a9edrkqqhylclwfdaf8sk78zgfkkc0ctvk4awq
Action:   increment(5) on tcr17kp5hrqhpdkwmukkrsfp6xft6y9l8j02rhxmvz
Fee:      0.00001295 TCN (205 bytes, priority Normal)
Broadcast OK. txid: 04a24a2dd4268cc4fa0ff1d3c54aba9f953f03abc8aad56a9e451a84da4c7e5b
$ thecoin-wallet contract view tcr17kp5hrqhpdkwmukkrsfp6xft6y9l8j02rhxmvz get
5
$ thecoin-wallet contract view tcr17kp5hrqhpdkwmukkrsfp6xft6y9l8j02rhxmvz last
tcr1a9edrkqqhylclwfdaf8sk78zgfkkc0ctvk4awq
```

- `thecoin-wallet contract deploy <file> [init args…] [--value TCN] [--max-fuel N] [--max-deposit TCN]`
- `thecoin-wallet contract invoke <address> <function> [args…] [--value TCN] [--max-fuel N] [--max-deposit TCN]`
- `thecoin-wallet contract view <address> <function> [args…]` — free, no transaction
- `thecoin-wallet contract program <address>` — balance, storage, deposit, interface
- `thecoin-wallet tx <txid>` — receipt: `success`, `error`, `fuel_used`, `logs` (events), `return_value`

The wallet simulates every call first: a call that would fail is never broadcast
(`contract call would fail: … (nothing was sent)`), and `max_fuel` is set to the measured
fuel × 1.3 + 5 000.

---

## Language reference

### File layout

```tccl
contract Shop                      # 1. header: always the first line of code

const MAX_NAME: int = 64           # 2. constants
state owner: address               # 3. state variables
state prices: map[text, int]
event Sold(item: text, buyer: address, price: int)   # 4. events

init():                            # 5. functions: init, action, view, fn
    owner = caller

action set_price(item: text, price: int):
    require caller == owner, "only the owner"
    require valid_name(item), "item names have 1 to 64 bytes"
    require price >= 0, "price cannot be negative"
    prices[item] = price           # a price of 0 removes the item

action buy(item: text) payable:
    require prices.has(item), "unknown item"
    require value == prices[item], "wrong price"
    send(owner, value)
    emit Sold(item, caller, value)

view price_of(item: text) -> int:
    return prices[item]

fn valid_name(item: text) -> bool:
    return len(item) >= 1 and len(item) <= MAX_NAME
```

- The first line of code is `contract Name`. One file = one contract.
- Blocks are defined by indentation (**spaces only**; a tab is a compile error). Lines inside `( )` and
  `[ ]` may continue on the next line. Comments start with `#`.
- Declarations (`const`, `state`, `event`, `init`, `action`, `view`, `fn`) can appear in any order;
  functions can call helpers declared later; constants can only use constants declared above.
- A contract needs at least one `init`, `action` or `view`.
- Every name is declared once per contract; there is no shadowing of locals.

**Keywords:** `contract const state event init action view fn payable let if elif else while for in
break continue return require send emit destroy pass true false and or not`

**Reserved built-in names** (cannot be declared): `caller value balance height self TCN int bool text
bytes address list map len sha256 blake3 to_bytes to_text to_int min max abs slice verify_ed25519
ring_verify address_of zero_address range`

### Types

| Type | Default | Literal | Notes |
|---|---|---|---|
| `int` | `0` | `42`, `-7`, `1_000_000` | signed 128-bit, checked arithmetic |
| `bool` | `false` | `true`, `false` | |
| `text` | `""` | `"hi\n"` | UTF-8; `len` counts bytes; escapes `\n \t \" \\` |
| `bytes` | empty | `0xdead_beef` | even number of hex digits |
| `address` | zero address | `address("tc1…")` | 20 bytes; prefix must match the network |
| `list[T]` | `[]` | `[1, 2, 3]` | `T` is any type except a map |
| `map[K, V]` | empty | — | **state variables only**; `K` scalar, `V` any non-map type |

Scalars (`int bool text bytes address`) can be compared with `==`/`!=`, used as map keys and given
initial values in `state`. There are no implicit conversions and no decimals: amounts are integers in
motes (1 TCN = `TCN` = 100 000 000 motes). `5tcn` is **not** valid source code — write `5 * TCN`
(the `5tcn` notation is accepted only for command-line arguments).

### Constants and state

```tccl
const FEE_BP: int = 25                        # computed at compile time
const TREASURY: address = address("tc1yfugpgq45fs8xe80x4jam2j2w2kqhk9qnp2umy")
state owner: address                          # default: zero address
state name: text = "Cloud Token"              # scalars may have a constant initial value
state holders: list[address]                  # stored item by item
state balances: map[address, int]
state history: map[int, list[int]]
```

Constants may use literals, earlier constants, operators and `address("…")`. Whole state lists and
maps cannot be assigned or copied; work with their items. **Default values are never stored:**
assigning `0`, `false`, `""`, empty bytes/list or the zero address deletes the storage entry (so
`m.has(k)` becomes `false`).

### Functions

| Kind | Header | Notes |
|---|---|---|
| `init` | `init(params) [payable]:` | optional, at most one, runs at deployment, no return value |
| `action` | `action name(params) [-> T] [payable]:` | entry point called by transactions; may return a value (receipt `return_value`) |
| `view` | `view name(params) -> T:` | free read-only query; must return a value; cannot assign state, `send`, `emit`, `destroy`, read `value`, or call an `fn` that does |
| `fn` | `fn name(params) [-> T]:` | private helper, only callable from code |

- The return type comes **before** `payable`: `action add(x: int) -> int payable:`.
- Only `action` and `init` can be `payable`; sending TCN to anything else fails
  (`function does not accept TCN (not payable)`).
- A function with a return type must return on every path: its last statement is a `return`, or an
  `if/else` whose branches all end with `return`.
- Entry points cannot be called from code. Recursion is allowed up to a call depth of 16.

### Statements

<!-- tccl-ctx: state owner: address | state balances: map[address, int] | state queue: list[address] | event Paid(to: address, amount: int) | action f(to: address, amount: int) payable: -->
```tccl
let fee: int = amount / 100              # locals: type and value are mandatory
balances[to] += amount - fee             # = += -= *= (+= also joins text/bytes)
require caller == owner, "only the owner"
if amount > 100 * TCN:
    pass
elif amount > 0:
    queue.push(to)
else:
    require false, "amount must be positive"
for i in range(0, 3):                    # range(start, end): start … end-1
    continue
for who in queue:                        # local or state list
    break
while len(queue) > 10:
    queue.pop()
send(to, fee)                            # move motes from the contract balance
emit Paid(to, fee)
```

- `require cond[, message]` — on failure the call stops and **every effect is reverted**
  (`requirement failed: message`; without a message: `requirement at line N failed`).
- `send(to, amount)` — fails with `invalid amount` (≤ 0) or `insufficient contract balance`. It never
  runs code at the receiver (contracts cannot call contracts: no re-entrancy).
- `emit Event(args…)` — arguments must match the declared fields; events go to the receipt.
- `destroy(to)` — actions only: deletes the contract and pays its balance **and storage deposit** to
  `to`. All list items and map entries must be removed first (scalars are cleared automatically).
- `return [value]`, `break`, `continue`, `pass`.
- An expression alone on a line must be a function call or `xs.pop()`.

### Operators

| Precedence (low → high) | Operators | Types |
|---|---|---|
| 1 | `or` | bool (short-circuit) |
| 2 | `and` | bool (short-circuit) |
| 3 | `not` | bool |
| 4 | `==` `!=` · `<` `<=` `>` `>=` | same scalar type · int |
| 5 | `+` `-` | int; `+` also text+text, bytes+bytes |
| 6 | `*` `/` `%` | int |
| 7 | unary `-` | int |
| 8 | `x[i]` `x.m(…)` `f(…)` | |

Overflow fails with `integer overflow`; `/` and `%` by zero fail; `/` truncates toward zero and `%`
takes the sign of the left operand (`-7 / 2 == -3`, `-7 % 2 == -1`). Chained comparisons
(`a < b < c`) are rejected. Conditions must be `bool` (no truthiness).

### Lists and maps

| Operation | Local list | State list |
|---|---|---|
| literal `[a, b]`, `[]` | ✓ | — |
| `xs[i]`, `xs[i] = v`, `xs[i] += v` | ✓ | ✓ |
| `xs.push(v)` | ✓ | ✓ |
| `xs.pop()` (statement or expression) | — | ✓ |
| `len(xs)` / `xs.len()` | ✓ | ✓ |
| `for x in xs:` | ✓ | ✓ |
| assign/copy the whole list | ✓ | — |

Local lists hold at most 4 096 items and 65 536 bytes. Maps: `m[k]` (default if absent),
`m[k] = v`, `m[k] += v`, `m.has(k)`, `m.remove(k)`. Maps cannot be iterated. To modify a list stored in
a map, copy it to a local, change it and store it back.

> The index of a compound assignment is evaluated exactly once: `m[next_id()] += 1` calls
> `next_id()` one time.

### Context values

| Name | Type | Meaning |
|---|---|---|
| `caller` | `address` | signer of the transaction (zero address inside views) |
| `value` | `int` | motes sent with the call (not available in views) |
| `balance` | `int` | contract balance in motes, including `value` |
| `height` | `int` | block height (≈ 1 block per minute) |
| `self` | `address` | this contract's address |
| `TCN` | `int` | 100 000 000 |

---

## Built-in functions

| Function | Result | Extra fuel |
|---|---|---|
| `len(list \| text \| bytes)` | `int` | — |
| `min(a, b)`, `max(a, b)`, `abs(a)` | `int` | — |
| `to_text(int)` | decimal `text` | — |
| `to_bytes(int \| address \| text \| bool \| bytes)` | `bytes` (int: 16 bytes big-endian; address: 20; bool: 1) | — |
| `to_int(bytes)` | unsigned big-endian `int` from ≤ 15 bytes | — |
| `slice(bytes, start, end)` | `bytes` | — |
| `sha256(bytes \| text)`, `blake3(bytes \| text)` | 32 `bytes` | 60 + 20 per 64 bytes |
| `verify_ed25519(pk, msg, sig)` | `bool` (false for malformed input) | 3 500 + 1 per 64 bytes |
| `ring_verify(ring: list[bytes], msg, sig, key_image)` | `bool` — linkable ring signature (bLSAG, Ristretto255), ≤ 64 keys | 5 000 + 10 000 per key |
| `address_of(pk: bytes)` | `address` of a 32-byte public key | — |
| `zero_address()` | `address` | — |
| `address("tc1…")` | compile-time address literal | — |

<!-- tccl-ctx: action f(pk: bytes, sig: bytes, amount: int): -->
```tccl
let msg: bytes = blake3(to_bytes(self) + to_bytes(caller) + to_bytes(amount))
require verify_ed25519(pk, msg, sig), "bad signature"
```

---

## Storage and the refundable deposit

- A contract's size (`state_bytes`) = compiled code + every storage entry (key + value).
- Required deposit = ⌈`state_bytes` / 1000⌉ × `storage_deposit_per_kb`
  (mainnet/testnet: 100 000 motes = 0.001 TCN per started kB).
- A deploy or a call that makes the state **grow** pays the missing deposit, up to the transaction's
  `max_deposit` (wallet default 1 TCN), otherwise it fails with
  `storage deposit of N motes exceeds max_deposit M`.
- A call that makes the state **shrink** receives `deposit × freed / old_size` back.
- `destroy(to)` pays the whole deposit to `to`.

---

## Fuel and fees

| Operation | Fuel |
|---|---|
| statement / expression | 2 / 1 |
| per 32 bytes of values read or combined | 1 |
| function call | 20 |
| storage read | 250 |
| storage write/delete | 400 + 4 per byte |
| `send` / `emit` / `destroy` | 300 / 100 + 1 per byte / 1 000 |
| hashes, signatures, rings | see built-ins |
| deploy: compile | 5 per source byte |
| invoke: load contract | 100 + 1 per 100 bytes of code |

Prices are calibrated to roughly 20 ns of CPU per fuel on a 2-vCPU server, so a completely full block
executes in about 1.2 s in the worst case. Example: `Counter.increment` uses 1 674 fuel in the simulator
(1 476 of it storage) and 1 777 on the network (+ loading).

**Fee** = (`base_fee` + ⌈size × `fee_per_kb` / 1000⌉ + ⌈`max_fuel` × `fee_per_kfuel` / 1000⌉) ×
congestion. Mainnet defaults: `base_fee` 1 000, `fee_per_kb` 10 000,
`fee_per_kfuel` 1 000 motes. You pay for the fuel you *reserve*. The congestion
surcharge is burned; `--priority low|normal|high|urgent` pays 1×, 1.25×, ≥ 2×, ≥ 4× the minimum.

Failed calls: the node refuses calls that would fail; if a call fails after being mined (state
changed meanwhile), **the fee is paid and every effect — storage, events, sends, the call's value —
is reverted**.

---

## Limits

| Limit | Value |
|---|---|
| source code | 48 000 bytes |
| compiled program | 262 144 bytes |
| deploy transaction / other transaction | 64 000 / 16 384 bytes |
| fuel per transaction | 10 000 000 |
| fuel per block (mainnet default) | 50 000 000 |
| fuel for views via the API / in the simulator | 2 000 000 / 5 000 000 |
| call depth | 16 |
| nesting of blocks, parentheses, types | 32 |
| operators of one precedence level in one expression | 64 |
| expression depth | 128 |
| functions / state variables / locals per function | 256 / 256 / 1 024 |
| text, bytes or list value | 65 536 bytes |
| items in a local list | 4 096 |
| events per call · arguments per call | 64 · 32 |
| ring size | 64 |

---

## Tools

### tccl

| Command | Description |
|---|---|
| `tccl check <file>` | compile and print the interface |
| `tccl abi <file>` | interface as JSON |
| `tccl run <file> [opts] deploy [args…]` | deploy in the simulator |
| `tccl run <file> [opts] call <fn> [args…]` | call an action |
| `tccl run <file> [opts] view <fn> [args…]` | call a view |
| `tccl ring keygen [--seed <hex32>]` | ring key pair and key image |
| `tccl ring sign --secret <hex> --ring <pk,…> --index <i> --message <0xhex>` | ring signature |

Run options: `--state <file>`, `--from <name>` (default `alice`), `--value 5tcn`, `--height <n>`,
`--contract <hex>`. Arguments: `42` or `2.5tcn` (int), `true`, `"text"`, `0xabcd`, `tc1…` or `@name`
(address parameters only), `[a, b]` (lists).

### Wallet: privacy pools (TCCL-PRIV-1)

Any contract exposing `denomination() -> int`, `deposits() -> int`, `key_at(int) -> bytes`,
`is_withdrawn(bytes) -> bool`, `message_for(address, address, int) -> bytes`, `deposit(bytes) payable`
and `withdraw(address, address, int, list[int], bytes, bytes)` works with:

- `thecoin-wallet privacy keygen [--key N]`
- `thecoin-wallet privacy deposit <pool> [--key N]`
- `thecoin-wallet privacy status <pool> [--key N]`
- `thecoin-wallet privacy withdraw <pool> --to <address> [--key N] [--ring-size 16] [--relayer <address>] [--fee TCN]`

See [`private_pool.tccl`](../../crates/tccl/examples/private_pool.tccl) and cookbook section 8.9.

### Node API

- `GET /api/v1/program/{address}` — metadata, storage, deposit, interface
- `POST /api/v1/program/{address}/view` — body `{"function": "get", "args": []}`
- `POST /api/v1/tx/simulate` — body `{"tx": "<hex>"}` → `success`, `error`, `fuel_used`, `required_fee`, `logs`, `return_value`
- `GET /api/v1/tx/{txid}` — transaction and receipt

---

## Runtime errors

| Error | Meaning |
|---|---|
| `requirement failed: …` | a `require` was false |
| `out of fuel` | fuel limit reached |
| `integer overflow` · `division by zero` | arithmetic error |
| `index I out of bounds (length N)` | bad index, `pop` on an empty list, bad `slice` |
| `value too large` | value over the size limits, or too many events |
| `call depth limit reached` | more than 16 nested calls |
| `unknown function 'f'` · `function 'f' cannot be called this way` | wrong name or kind |
| `wrong arguments: …` | argument count/types, ring too large, `to_int` over 15 bytes |
| `function does not accept TCN (not payable)` | value sent to a non-payable function |
| `state cannot be modified in a view` | run-time guard |
| `invalid amount` · `insufficient contract balance` | bad `send` |
| `contract cannot be destroyed while it still has storage (N entries)` | empty lists/maps first |

Compile errors have the form `file:line L:C: message`, e.g. `expected int, found text`,
`a view cannot change state`, `function 'f' must return a int on every path`. The cookbook lists all of
them with fixes.

---

## Security checklist

- **Access control:** every action that moves money checks `caller`; roles are set in `init`.
- **Money flows:** each `send` has a clear trigger; settlement runs once (flags or delete-before-send);
  order checks → effects → interactions; pay from your own counters, not `balance` (anyone can
  increase a contract's balance with a plain transfer).
- **Integers:** amounts in motes; multiply before dividing; reject negative inputs.
- **Denial of service:** no loop over lists others can grow without a bound; prefer pull payments.
- **Storage:** remember that default values delete entries; composite keys must be unambiguous.
- **Time and randomness:** use `height`; nothing derived from chain data is random — use commit–reveal.
- **Front-running:** mempool transactions are public; bind signed messages to `self`, recipient,
  amounts and fees; store key images before paying.
- **Cryptography:** `verify_ed25519`/`ring_verify` return `false` on bad input — always `require` them.
- **Process:** test every `require`, run on testnet, get a review; deployed code cannot be changed.

---

## Example contracts

All examples compile and are exercised by `cargo test -p tccl` ([tests](../../crates/tccl/tests/examples.rs)).

| Contract | Shows |
|---|---|
| [`counter.tccl`](../../crates/tccl/examples/counter.tccl) | state, actions, views, events |
| [`shop.tccl`](../../crates/tccl/examples/shop.tccl) | every kind of declaration, payable, helpers |
| [`tip_jar.tccl`](../../crates/tccl/examples/tip_jar.tccl) | `init`, payable, owner permissions |
| [`token.tccl`](../../crates/tccl/examples/token.tccl) | maps, allowances, composite keys |
| [`crowdfund.tccl`](../../crates/tccl/examples/crowdfund.tccl) | deadlines, refunds |
| [`escrow.tccl`](../../crates/tccl/examples/escrow.tccl) | roles, state machine, `destroy` |
| [`poll.tccl`](../../crates/tccl/examples/poll.tccl) | list arguments, state lists, bounded loops |
| [`savings.tccl`](../../crates/tccl/examples/savings.tccl) | time locks, storage refunds |
| [`treasury.tccl`](../../crates/tccl/examples/treasury.tccl) | M-of-N approvals, parallel maps |
| [`names.tccl`](../../crates/tccl/examples/names.tccl) | text keys, validation, expiry |
| [`private_pool.tccl`](../../crates/tccl/examples/private_pool.tccl) | ring signatures, private payments |
