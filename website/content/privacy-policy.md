---
title: 'Privacy'
PageTitle: 'Privacy · Tallybook'
description: 'Tallybook keeps your books on your computer. What the app and this website send anywhere, and why.'
type: 'terms'
updated: 2026-10-31
priority: 0.1
cta: false
---

Tallybook is made by Jason Sutter ("we" and "us" in this policy). The short
version: **your books never leave your computer unless you send an
encrypted copy somewhere yourself.** We don't keep your transactions, budgets,
invoices or tax figures, and we don't want to.

## The Tallybook app

Your books are stored in a single `.tallybook` file on your computer. The app
has no account, no analytics and no crash reporting. It only talks to the
internet in these cases:

- **Checking for updates.** A few times a day Tallybook asks GitHub, where its
  releases are published, whether a newer version exists. GitHub sees your IP
  address, as it would for any download.
- **Activating a license.** When you paste in a license key, Tallybook sends
  that key and a name for this computer to Lemon Squeezy to confirm it. The
  result is stored on your computer. Removing the license from a computer tells
  Lemon Squeezy to free the key.
- **Bank sync, through Tallybook Connect.** If you connect a bank, Tallybook
  talks to Tallybook Connect, a small service run by Tallybook, which talks to
  Plaid. See the next section.
- **Older bank connections.** Connections made in earlier versions, through
  an outside bank-data service you signed up for yourself, or through Plaid with your own keys, talk to that service directly
  from your computer, using your own account with them. Their privacy policies
  apply. Those access tokens are kept in your system keychain, not in the book
  file.
- **Cloud copies you turn on.** Copies go only to the folder or Git repository
  you choose, and only ever encrypted.

## Bank sync and Tallybook Connect

Bank sync is the one feature that uses a Tallybook service. Your bank password
goes only to Plaid's own sign-in page. We never see it.

- **What Connect processes.** Your bank data, such as accounts and transactions,
  passes through Tallybook Connect on its way from Plaid to your computer. It is
  held in memory only while it is in transit. We don't store it, and our logs
  don't contain it.
- **What Connect stores,** in an encrypted database: your bank access token
  (the key that lets Connect ask Plaid for your transactions), your license
  key and a hash of it, and timestamps such as when the key was last seen
  active. Nothing from
  your books, and no balances or transactions.
- **Why.** To sync your banks, and to check your plan with Lemon Squeezy so Connect
  can disconnect your banks when the plan ends.
- **Where it runs.** Connect runs on a server we control. Its traffic reaches
  it through our network provider (currently Cloudflare), which carries it only
  to deliver it.
- **Plaid.** Plaid acts as our processor for bank connections. Plaid's own
  [end user privacy policy](https://plaid.com/legal/#end-user-privacy-policy)
  describes what it collects from you when you sign in to your bank.
- **Deletion.** What Connect stores is deleted when you disconnect a bank, or
  within about 30 days after a plan ends.

## Buying a plan

Purchases are handled by Lemon Squeezy, which acts as our reseller and
processes your payment. We receive your name, email address and order details
so we can send your key and help with your purchase or renewal. We never see your card
details. Lemon Squeezy's own privacy policy covers what they collect.

We use your email to deliver your license key and to answer you. We don't sell or
share it, and we won't send you marketing unless you ask for it.

## This website

This website has no advertising and no cookies. We count visits with Zero Trust Analytics (ztas.io). It sets no cookies, stores nothing on your device and does not store your IP address. For each page view it records the page, the site that sent you here, your browser, device type and operating system, an approximate location (country, region and city), and a one-way code that changes every day so a visit isn't counted twice. Fonts and images
are served from this site. Our host keeps standard server logs (such as IP
address and the page requested) for a short time to keep the site running and
secure. Downloads are served by GitHub.

## Your choices

You can export or delete your books at any time; they're files you own. To ask
about the information we hold from a purchase, or to have it deleted, reply to
your purchase receipt email; replies reach us privately. Please don't put
personal details in a public GitHub issue.

## Changes

If this policy changes, we'll update this page and the date at the top.
