# Demo of SwapBytes

This is an example interaction between two SwapBytes users on the same local machine.

---

### 1. New sessions

Both users have detected each other, but lost connection (as seen by the `[✗]`).

![Screenshot 1](demo_screenshots/1.png)

### 2. Manual connection

One user runs `/me` to get their `multiaddr`. The other user runs `/ping <multiaddr>` to initiate a lasting connection.

![Screenshot 2](demo_screenshots/2.png)

### 3. Set nicknames

Both users set their nicknames using `setname <name>` and one user uses `/who <name>` on the other user.

![Screenshot 3](demo_screenshots/3.png)

### 4. Open private chat

Both users use `/chat <name>` to open private chats with each other and they talk.

![Screenshot 4](demo_screenshots/4.png)

### 5. File offer

One user offers the other a PDF file using `/offer <file>`. The other user makes sure they used `/setdir <dir>` before accepting the offer.

![Screenshot 5](demo_screenshots/5.png)

### 6. File transfer

By running `/accept`, the file transfer starts notifying both parties. The receiver can see the progress as it happens.

![Screenshot 6](demo_screenshots/6.png)

### 7. File transfer completion

The transfer completed effectively and the receiver says "thanks!".

![Screenshot 7](demo_screenshots/7.png) 