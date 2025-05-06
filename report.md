# SwapBytes Assignment Report

**Course:** COSC 473 – Decentralised Applications on the Web  
**Assignment:** Assignment 2 - SwapBytes  
**Student:** Haig Bishop  
**Repository:** https://github.com/HaigBishop/swapbytes 

## 1. Introduction

This report describes SwapBytes developed by Haig for Assignment 2 of COSC 473. My goal was to create a visually pleasing and easy-to-use CLI/TUI for P2P file transfer and chats.

The resulting application utilised `ratatui` for the TUI, which displays a "Users" pane listing all active users, a "Chat" pane which enables both global and private chats, and a "Console" pane which allows to user to run commands. P2P features such as connectivity, messaging and file transfer are implemented using `libp2p`.

## 2. How SwapBytes Works

SwapBytes allows users to connect, chat and transfer files in a decentralised manner. It leverages multiple techniques:

*   ...

For more practical information on how SwapBytes works, see README.md and demo.md on the [GitHub repo](https://github.com/HaigBishop/swapbytes).

## 3. Challenges and Solutions

Developing a P2P application presents unique challenges, particularly around peer discovery and connectivity.

...

## 4. Requirements & Bonus Features

*   **Basic Requirements:** All basic requirements appear to have been met:
    *   CLI/TUI interface (implemented with `ratatui` TUI).
    *   Users get auto-assigned usernames, then can change them with `/setname`.
    *   There is a global chat where all users can see all messages.
    *   Direct message chats can be opened to send/receive private messages.
    *   File sending via request/response (`/offer`, `/accept`, `/decline` ).
    *   Peer discovery: mDNS is implemented for peer discovery, and also rendezvous servers can be set up without too much difficulty. Additionally, the manual `/ping <multiaddr>` command provides a mechanism that *can* work for cases in which mDNS fails (more info in README.md).
    *   Full documentation is found in `README.md`.
*   **Bonus Features:**
    *   **Advanced UI using ratatui:** This bonus feature has been implemented, providing a nice user experience with 3 panes, scrolling, multiple user input options and a graceful exit.
    *   **Multiple "Extra Features":**  
        *   User list - Dynamic list of active users.
        *   Directory safety - Manual setting of download directory validates paths.
        *   Duplicate nickname handling - Allowed, but warns user when duplicate.
        *   Visibility control - Users can toggle visibility using `/show` and `/hide`.
        *   Self info command - Users can run `/me` to see info on themselves.
        *   User info command - Users can run `/who <name>` to see info on another user.
        *   Help command - Users can run `/help` to see all commands they can run.

## 5. Grade Estimate and Justification

**Estimated Grade:** A

**Justification:** 

*   The project  implements all the specified basic requirements for the SwapBytes application. It features a functional P2P system for chat and file offering built on `libp2p` and Rust. Crucially, it goes beyond the basic requirements by implementing an **advanced `ratatui` TUI** and a few extra features listed above. 
*   The codebase is well-structured into modules, and the `README.md` provides good documentation, instructions, demonstration, and discussion of potential issues and workarounds. 
*   ...

*   The overall quality, completeness of basic features, inclusion of a bonus features, and thorough documentation justify an A grade.
