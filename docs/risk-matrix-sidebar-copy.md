# Risk Matrix sidebar: original and shortened copy

## Scope

This note covers the right-hand **Compliance & Risk History** sidebar after a
company or agency is selected. It does not change the matrix heading, the
entity description, severity labels, sources, or the underlying incident data.

## Before

Each incident rendered its complete `event.desc` text immediately. A long
description therefore occupied the full sidebar before the reader could reach
the image, sources, or later incidents. There was no collapsed state and no
way to expand or reduce an individual incident.

## Now

The original incident description is kept unchanged as the source text. The
sidebar initially shows a short preview:

- Up to 180 characters.
- It ends at the first sentence boundary within that range when possible.
- Otherwise it ends at a word boundary with an ellipsis.
- A **Read details** control reveals the exact original description.
- **Show less** returns that incident to the short preview.

The open/closed state is local to the selected entity and resets when a
different entity is chosen. Incident images also use a fixed 16:9 frame, so a
large image cannot make the sidebar look disproportionately long.

## Meaning of the change

This is a presentation-only shortening. No claim, citation, incident title,
year, severity, or source link was deleted or rewritten. The full original
content remains available from **Read details**.
