# Calendar and contacts

| Format     | Extensions             | Notes |
|------------|------------------------|-------|
| iCalendar  | `.ics` `.ical` `.ifb`  | Calendar of events and todos |
| vCard      | `.vcf` `.vcard`        | One or more contacts |

These are the two IETF "vObject" text formats, exported by Google Calendar, Apple Calendar,
Outlook, and most address books. peek also recognises them by content, so an extension-less file
(or piped stdin) still opens here when it starts with a `BEGIN:VCALENDAR` or `BEGIN:VCARD` line.

Both open on a rendered read view, with the raw text as a second view (cycle with Tab). `--plain`
skips the rendered view and opens straight on the source. As with every file, `x` opens the
[hex dump](./binary.md), `i` the [Info screen](../viewer/info-screen.md), and `h` / `?` the help.

## iCalendar — an agenda

The rendered view lists each event and todo in turn: a summary heading, the date and time (with
the time zone shown as written — peek doesn't convert zones), location, a readable recurrence rule
(`Weekly on Mon, Tue, Wed, Thu, Fri, 40 times`), organizer and attendees, status, categories, and
the description. Todos show their due date and completion status.

The **Info** view summarises the calendar: its name, the event and todo counts, the date range
events span, the format version, and the producing application.

## vCard — contact cards

The rendered view shows one grouped card per contact: the display name, nickname, organisation,
title, every email / phone / address with its type label (work, home, cell, …), website, birthday,
categories, and notes. Both vCard 3.0 and 4.0 cards are handled.

The **Info** view shows the contact count and the vCard version.
