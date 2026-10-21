# User guide

Sign in as the owner. Add a feed URL or a site URL; autodiscovery finds RSS,
Atom, or JSON feeds. Organize feeds into folders, import/export OPML, read the
unread/all/starred views, and search article text.

![Reader](screenshots/reader.png)

Selecting an article marks it read.

![Article](screenshots/article.png)

Keyboard: `j`/`k` move between articles, `s` stars, `m` toggles read.

![Starred](screenshots/starred.png)

## Reader apps

Create a named app credential in **Subscriptions & apps**:

- FreshRSS/Google Reader URL: `https://your-host/api/greader.php`
- Fever URL: `https://your-host/api/fever.php`
- Fever API key: `MD5(username:password)` — most clients compute it for you;
  Read You wants URL + username + password as entered in the credential.

Copy the password immediately; it is not shown again. Revoke a credential to
disconnect only that app. Verified with NetNewsWire (Google Reader API) and
Read You (Fever API).
