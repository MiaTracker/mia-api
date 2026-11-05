# Mia

A personal movie and TV show tracker: each user keeps their own library of media, viewing logs and watchlist, and can chat with an LLM assistant.

## Language

### Assistant

**Conversation**:
A titled, ordered thread of chat messages between one user and the LLM assistant; created together with its first **Exchange**.
_Avoid_: Chat, session, thread

**Exchange**:
One user **Message** together with every tool and assistant **Message** that follows it, up to the model's final answer; succeeds or fails as a whole.
_Avoid_: Reply, turn, round

**Message**:
One stored entry in a **Conversation**'s history, as exchanged with the model; its **Role** is user, tool or assistant.
_Avoid_: Prompt, turn

**Role**:
Who produced a **Message**: the person (`user`), a tool returning results (`tool`), or the model (`assistant`).
_Avoid_: System (reserved for the system prompt)

**Tool result**:
A **Message** with role `tool`, carrying the outcome of tool calls back to the model; never shown to the person as something they wrote.

**Tool round**:
One **LLM call** that requested tools, together with the **Tool result** message answering all its calls; an **Exchange** holds as many as the model needs before its final answer, up to a limit.

**Partial message**:
The assistant **Message** of an **LLM call** that failed after output began to arrive, holding whatever arrived; kept in the failed **Exchange** so the person can see it until a **Retry** clears it.

**Widget**:
Rich inline content (media cards, a map) inside an assistant **Message**, drawn by the client from a display tool call's input at that point in the message.

**LLM call**:
One request to the model provider, successful or failed; every request ever sent is recorded as one, kept as a permanent usage record that is never deleted, even after its content or its user is.
_Avoid_: Request, completion, generation

**Stream end**:
The last event of every SSE stream the server ends; it says nothing more will come, not that the run succeeded.

**Retry**:
Resuming a failed **Exchange** from the point it failed: its **Partial message** is discarded and the failed **LLM call** is made again from scratch, keeping the Messages that already succeeded. The failed LLM call stays in the ledger. The Exchange also moves to the end of the **Conversation**: its creation time becomes the time of the Retry.

**Regenerate**:
Discarding an **Exchange**'s tool and assistant Messages and running it again from its user Message.

**Truncate**:
Deleting every **Exchange** after a given one, so history stays a single line; happens when an earlier Exchange is edited or regenerated.
_Avoid_: Branch (conversations do not branch)

## Relationships

- A **User** has zero or more **Conversations**; deleting the user deletes them
- A **Conversation** contains an ordered list of **Exchanges**
- An **Exchange** contains an ordered list of **Messages**, starting with exactly one user **Message**
- Every assistant **Message** is produced by exactly one **LLM call**; an LLM call produces at most one: a complete or **Partial message** once output arrives, none if it fails before any does
- A **Conversation**'s title is produced by its own **LLM call**, outside any Exchange
- A **Conversation**'s title is delivered on the stream of the request that created it; other streams never carry it
- A **User** owns all their **LLM calls**, which outlive the Conversations, Exchanges and User they belonged to
- An assistant **Message** that requests tools is followed by exactly one **Tool result** message answering all its calls
- The history sent to the model leaves out failed **Exchanges**; the person still sees them in the **Conversation**

## Flagged ambiguities

- "system" was proposed for tool responses — resolved: the role is `tool`; "system" refers only to the system prompt.
- "message" was used both for a stored **Message** and for the whole answer the person sees — resolved: the latter is an **Exchange**.
