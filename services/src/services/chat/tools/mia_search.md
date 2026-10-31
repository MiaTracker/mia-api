Searches the user's own library (movies and series they added to mia) and returns matching items with their mia id. Use it to answer questions about what the user owns, has rated or has watched. It never searches TMDB. Pass the mia ids to mia_details for full details.

Input: {"query": "<query>"}. A query has up to three parts separated by " : " (space, colon, space):

    title : filter : sort

Each part is optional from the right. The title may be empty; to give a filter or sort without a title start the query with " : ". To give a sort without a filter leave the filter empty with two spaces between the colons ("title :  : stars desc"). To search for a title that itself contains " : " write it as " \: " (raw query text; inside the JSON tool input the backslash is doubled, " \\: "). The empty query "" returns the first items of the library.

TITLE: free text, case-insensitive. Matches items with any title (also alternative titles) that starts with the text, also counting a leading "The " or "A " as optional. Not a filter expression; nothing in it is an error.

FILTER: conditions on fields. Unknown field names and malformed conditions are errors, but some mistakes are NOT reported and silently give wrong results, so write exactly as below: and / or / true / false / null are lowercase only ("AND" is not a keyword); write true / false / null without quotes (watched = "false" would count as true); join every condition to the next with and / or (text after a complete condition is ignored, so "stars > 5 rated = true" or "stars > 5 AND rated = true" runs as just "stars > 5").
  comparison:  field OP literal      OP is one of  =  !=  <  <=  >  >=
  range:       low OP field OP high  e.g. 1990-01-01 <= release_date < 2000-01-01 (no null in a range)
  combine:     and, or, parentheses. ALWAYS use parentheses when mixing and with or (without them "a and b or c" means "a and (b or c)").
Literals: "text" in double quotes; numbers 7 or 7.5; dates as YYYY-MM-DD with NO quotes; true / false; null. Do not compare with null; use rated / has_source / on_watchlist instead.

Fields (all are the user's own data) and their literals:
  stars                 number 0 to 10: the user's rating (decimals allowed, e.g. 7.5). Unrated items match only !=; use rated for them.
  rated                 true / false: whether the user rated it. Only =, !=.
  watched               true / false: whether the user logged at least one viewing. Only =, !=.
  times_watched         integer: number of logged viewings. All operators.
  type                  "movie" or "series". Only =, !=.
  has_source            true / false: whether it has any source (where to watch it). Only =, !=.
  number_of_sources     integer: number of sources. All operators.
  has_source_with_name  "text": has a source with exactly this name, case-insensitive. Only =, !=.
  has_tag               "text": has a tag with exactly this name, case-insensitive. Only =, !=.
  has_genre             "text": has a genre with exactly this name, case-insensitive (e.g. "science fiction"). Only =, !=.
  language              "text": original language by English name, native name or ISO 639-1 code, case-insensitive (e.g. "english", "en"). Only =, !=.
  id                    integer: the mia id. All operators.
  tmdb_id               integer: the TMDB id. All operators.
  release_date          date: movie release date or series first air date. All operators. Items without a date do not match < <= > >= =.
  date_added            date: the day the user added it to the library. All operators.
  on_watchlist          true / false: whether it is on the user's watchlist. Only =, !=.
  tmdb_rating           number 0 to 10: the TMDB average vote. All operators.
Fields marked "Only =, !=" reject < <= > >=.
For has_tag, has_source_with_name, has_genre and language the user's exact names are needed. If such a filter returns nothing, the name may be wrong or spelled differently; try another spelling or drop the filter and look at the results.

SORT: field, then optional direction: asc, ascending, desc or descending (default asc).
Sort fields: stars, watched, times_watched, type, has_source, number_of_sources, release_date, date_added, tmdb_rating. Without a sort, results come in the library's default order, by title.

Examples (the value of "query" as a JSON string; mind the spaces around the colons):
  "alien"                                                  items whose title starts with "alien"
  " : type = \"series\" and watched = false"                 unwatched series (empty title)
  " : has_genre = \"comedy\" and stars >= 8 : stars desc"    best-rated comedies first (filter + sort)
  " : 1990-01-01 <= release_date < 2000-01-01"             released in the 1990s (range form, date literals)
  " : (has_tag = \"scary\" or has_tag = \"horror\") and rated = false"
  "matrix : type = \"movie\" : release_date asc"            movies starting with "matrix", oldest first (all three parts)
  " :  : date_added desc"                                  no title, no filter, newest additions first
  " : date_added > 2026-01-01 and on_watchlist = true"

Result: {"results": [{"id", "type" ("movie" or "series"), "title", "release_year", "overview", "tmdb_id", "stars"}]}. Missing values are null; "stars" is the user's own rating. Results are capped at a fixed number and there is no paging: narrow the query (filter, sort) rather than expecting more. An invalid query returns an error message naming what is wrong; fix the query and retry.
