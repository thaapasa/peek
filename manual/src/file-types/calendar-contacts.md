# Calendar and contacts

| Format    | Extensions            | Notes                              |
| --------- | --------------------- | ---------------------------------- |
| iCalendar | `.ics` `.ical` `.ifb` | Calendar of events and to-do items |
| vCard     | `.vcf` `.vcard`       | One or more contacts               |

iCalendar and vCard are the two "vObject" text formats from the Internet Engineering Task Force
(IETF). Google Calendar, Apple Calendar, Outlook, and most address books export them. peek also
recognizes them by their content. A file without an extension, or piped stdin, still opens here if
it starts with a `BEGIN:VCALENDAR` or `BEGIN:VCARD` line.

Both formats open in a rendered read view. The raw text is a second view. To switch between them,
press `Tab`. To skip the rendered view and open the source directly, use `--plain`. As with every
file, press `x` to open the [hex dump](./binary.md), `i` to open the
[Info screen](../viewer/info-screen.md), and `h` or `?` to open the help.

## iCalendar agenda

The rendered view lists each event and to-do item (`VTODO`) in turn. For each one, it shows the
following:

- A summary heading.
- The date and time. The time zone shows as written, because peek doesn't convert time zones.
- The location.
- A readable recurrence rule, such as `Weekly on Mon, Tue, Wed, Thu, Fri, 40 times`.
- The organizer and attendees.
- The status.
- The categories.
- The description.

To-do items also show their due date and completion status.

The **Info** view sums up the calendar. It shows the calendar name, the counts of events and to-do
items, the date range that the events cover, the format version, and the app that produced the file.

## vCard contact cards

The rendered view shows one grouped card for each contact. A card shows the display name, nickname,
organization, title, website, birthday, categories, and notes. It also shows every email address,
phone number, and postal address, each with its type label, such as work, home, or cell. peek
handles both vCard 3.0 and 4.0 cards.

The **Info** view shows the contact count and the vCard version.
