// Sample data for the UI kit. Voice modeled on the brief: declarative,
// specific, willing to say what isn't known. No emoji. No exclamation marks.

const RECENT_REPORTS = [
  { id: 144, title: "The Bond Vigilantes Return",          date: "Sun · Apr 14", read: "14m", isNew: true },
  { id: 143, title: "Energy Re-rating, Round Two",         date: "Sun · Apr 07", read: "21m" },
  { id: 142, title: "A Quiet Week, Honestly",              date: "Sun · Mar 31", read: "9m", current: true },
  { id: 141, title: "Disinflation Without a Recession",    date: "Sun · Mar 24", read: "18m" },
  { id: 140, title: "The Energy Re-rating Is Real",        date: "Sun · Mar 17", read: "16m" },
  { id: 139, title: "Earnings Season, Quietly",            date: "Sun · Mar 10", read: "11m" },
  { id: 138, title: "Powell's Conditional Pivot",          date: "Sun · Mar 03", read: "19m" },
  { id: 137, title: "The Yen, Again",                      date: "Sun · Feb 25", read: "13m" },
  { id: 136, title: "What the Curve Is Saying",            date: "Sun · Feb 18", read: "22m" },
  { id: 135, title: "Magnificent Seven, Less Magnificent", date: "Sun · Feb 11", read: "17m" },
  { id: 134, title: "China Tape Bombs",                    date: "Sun · Feb 04", read: "15m" },
  { id: 133, title: "Why We Were Wrong on Rates",          date: "Sun · Jan 28", read: "20m" },
];

const WATCHLIST = [
  { name: "S&P 500",         last: "4,392.18",  wk: "+1.42%",  ytd: "+8.1%"  },
  { name: "WTI Crude",       last: "73.46",     wk: "−0.83%",  ytd: "+11.2%" },
  { name: "US 10Y Yield",    last: "4.31%",     wk: "+0.06",   ytd: "+0.32"  },
  { name: "US 2Y Yield",     last: "4.69%",     wk: "+0.04",   ytd: "+0.18"  },
  { name: "DXY",             last: "104.27",    wk: "+0.21%",  ytd: "+1.8%"  },
  { name: "Gold",            last: "2,318.40",  wk: "+0.42%",  ytd: "+12.4%" },
  { name: "BTC/USD",         last: "67,140",    wk: "−2.10%",  ytd: "+58.6%" },
];

const INBOX_ITEMS = [
  { id: 7, title: "Q1 letters — value managers",  source: "PDF · 12 files",  added: "Apr 12", tag: "letters" },
  { id: 6, title: "BIS quarterly review",          source: "PDF · 142 pp",    added: "Apr 09", tag: "central-bank" },
  { id: 5, title: "Note: rate-vol vs equity-vol",  source: "User note",        added: "Apr 08", tag: "research" },
  { id: 4, title: "Powell — Senate testimony",     source: "Transcript",       added: "Apr 05", tag: "central-bank" },
  { id: 3, title: "10-K — selected energy names",  source: "PDF · 8 files",    added: "Apr 02", tag: "filings" },
  { id: 2, title: "China credit data, Mar",        source: "PBoC release",     added: "Apr 01", tag: "data" },
  { id: 1, title: "ECB minutes — March",           source: "PDF · 38 pp",      added: "Mar 28", tag: "central-bank" },
];

window.MS_DATA = { RECENT_REPORTS, WATCHLIST, INBOX_ITEMS };
