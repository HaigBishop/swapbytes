

# COSC 473 – Decentralised Applications on the Web

## Assignment 2 - SwapBytes

### Introduction

The goal of this assignment is to create a P2P file bartering application called **SwapBytes** using **libp2p**.

Users will be able to offer files in exchange for others — e.g., swapping Machine Learning notes for COSC 473 tutorial notes. This assignment demonstrates core concepts of libp2p and P2P networking: peer discovery, establishing connections, data exchange, and file transfer.

📅 **Due date**: Monday, April 28 (end of day)

---

### Basic Requirements

* Implement a simple **CLI user interface** to send/receive messages and files.

* Peers have **user-defined nicknames** that others can use for direct messaging or file sharing.

* A **pubsub chat room** for proposing file swaps: what users want vs. what they offer.

* Peers who connect for a swap in the chat room should be able to **direct message** each other.

* After agreeing to swap files, peers should **send files** using a request/response pattern.

* Implement a **bootstrap mechanism** for peer discovery:

  * Use **mDNS** for local discovery.
  * Also support connecting to peers **outside the local network** (e.g., using a **rendezvous server**).
  * **NAT traversal** (hole punching) is *not* required.

* **Full documentation** required:

  * How to run your program (including CLI parameters, bootstrapping).
  * Step-by-step examples in the repo’s `README.md`.
  * All commands for chat, file sharing, etc. must be clearly documented.

---

### Bonus Features

* **Themed rooms**:

  * Users can create and join rooms.
  * Rooms are shared among peers upon joining.

* **Reputation system**:

  * Peers can vote +1 or -1 on others (e.g., if a deal falls through).
  * Rankings are shared via **kademlia DHT**.

* **Peer discovery via kademlia DHT** and/or hole punching.

* **End-to-end encryption**:

  * Encrypt files with the recipient’s public key.
  * Recipient decrypts upon receipt.

* **Advanced UI** using [`ratatui`](https://github.com/ratatui-org/ratatui).

* Other creative additions that demonstrate:

  * A new Rust concept or crate.
  * A libp2p feature not otherwise required.

---

### Submission Requirements

* Submit your Rust code as a **repository on eng-git**, and **add user `bta47`**.
* Include a `README.md` with **clear instructions** to run your app.
* Upload a **brief report (1–2 pages)** to Learn:

  * Explain how the application works.
  * Describe challenges and your solutions.
  * Include a link to your repository.
  * Estimate your grade and justify it.

---

### Grading

Marks will be based on a **holistic evaluation**, including:

* ✅ **Correctness**: Does it work?
* 🧠 **Code Quality**: Structure, clarity, comments.
* 📄 **Documentation**: Clear instructions and report.
* 💡 **Creativity**: Bonus features, novel ideas.

#### Rough Guidelines:

* **C or lower**: Incomplete or broken basic features.
* **B**: Complete basic features with basic docs.
* **A**: Bonus features or strong creativity/effort shown.

---