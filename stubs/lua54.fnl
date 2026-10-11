;;; Lua 5.4 Built-ins stub
;;; Based on the Lua 5.4 Reference Manual

(global _G {})
(global _VERSION "Lua 5.4")
(global arg {})

(fn assert [v ?message]
  "Issues an error when the value of its argument v is false (i.e., nil or false); otherwise, returns all its arguments.")

(fn collectgarbage [?opt ?arg]
  "This function is a generic interface to the garbage collector.")

(fn dofile [?filename]
  "Opens the named file and executes its contents as a Lua chunk.")

(fn error [message ?level]
  "Terminates the last protected function called and returns message as the error message.")

(fn getmetatable [object]
  "If object does not have a metatable, returns nil.")

(fn ipairs [t]
  "Returns three values: an iterator function, the table t, and 0.")

(fn load [ld ?source ?mode ?env]
  "Loads a chunk.")

(fn loadfile [?filename ?mode ?env]
  "Similar to load, but gets the chunk from file ?filename.")

(fn next [table ?index]
  "Allows a program to traverse all fields of a table.")

(fn pairs [t]
  "If t has a metamethod __pairs, calls it with t as argument and returns the first three results from the call.")

(fn pcall [f ?arg1 & ...]
  "Calls function f with the given arguments in protected mode.")

(fn print [& ...]
  "Receives any number of arguments and prints their values to stdout.")

(fn rawequal [v1 v2]
  "Checks whether v1 is equal to v2, without invoking any metamethod.")

(fn rawget [table index]
  "Gets the real value of table[index], without invoking any metamethod.")

(fn rawlen [v]
  "Returns the length of the object v, which must be a table or a string.")

(fn rawset [table index value]
  "Sets the real value of table[index] to value, without invoking any metamethod.")

(fn require [modname]
  "Loads the given module.")

(fn select [index & ...]
  "If index is a number, returns all arguments after argument number index.")

(fn setmetatable [table metatable]
  "Sets the metatable for the given table.")

(fn tonumber [e ?base]
  "Tries to convert its argument to a number.")

(fn tostring [e]
  "Receives a value of any type and converts it to a string in a reasonable format.")

(fn type [v]
  "Returns the type of its only argument, coded as a string.")

(fn warn [msg & ...]
  "Emits a warning with a message composed by the concatenation of all its arguments.")

(fn xpcall [f msgh ?arg1 & ...]
  "This function is similar to pcall, except that it sets a new message handler msgh.")

;;; Modules

(global coroutine {})
(fn coroutine.close [co]
  "Closes coroutine co, closing all its pending to-be-closed variables and putting the coroutine in a dead state.")
(fn coroutine.create [f]
  "Creates a new coroutine with body f. Returns this new coroutine, an object with type \"thread\".")
(fn coroutine.isyieldable [?co]
  "Returns true when the coroutine co can yield.")
(fn coroutine.resume [co & ...]
  "Starts or continues the execution of coroutine co.")
(fn coroutine.running []
  "Returns the running coroutine plus a boolean, true when the running coroutine is the main one.")
(fn coroutine.status [co]
  "Returns the status of coroutine co, as a string: \"running\", \"suspended\", \"normal\", or \"dead\".")
(fn coroutine.wrap [f]
  "Creates a new coroutine with body f; returns a function that resumes the coroutine each time it is called.")
(fn coroutine.yield [& ...]
  "Suspends the execution of the calling coroutine.")

(global debug {})
(fn debug.debug []
  "Enters an interactive mode with the user, running each string that the user enters.")
(fn debug.gethook [?thread]
  "Returns the current hook settings of the thread.")
(fn debug.getinfo [f ?what]
  "Returns a table with information about a function.")
(fn debug.getlocal [f local]
  "Returns the name and the value of the local variable with index local of the function or level f.")
(fn debug.getmetatable [value]
  "Returns the metatable of the given value or nil if it does not have a metatable.")
(fn debug.getregistry []
  "Returns the registry table.")
(fn debug.getupvalue [f up]
  "Returns the name and the value of the upvalue with index up of the function f.")
(fn debug.getuservalue [u ?n]
  "Returns the n-th user value associated to the userdata u plus a boolean false if the userdata does not have that value.")
(fn debug.sethook [hook mask ?count]
  "Sets the given function as a debug hook.")
(fn debug.setlocal [level local value]
  "Assigns the value value to the local variable with index local of the function at level level.")
(fn debug.setmetatable [value table]
  "Sets the metatable for the given value to the given table (which can be nil).")
(fn debug.setupvalue [f up value]
  "Assigns the value value to the upvalue with index up of the function f.")
(fn debug.setuservalue [udata value ?n]
  "Sets the given value as the n-th user value associated to the given udata.")
(fn debug.traceback [?message ?level]
  "Returns a traceback of the call stack. An optional message string is appended at the beginning of the traceback.")
(fn debug.upvalueid [f n]
  "Returns a unique identifier (as a light userdata) for the upvalue numbered n from the given function.")
(fn debug.upvaluejoin [f1 n1 f2 n2]
  "Make the n1-th upvalue of the Lua closure f1 refer to the n2-th upvalue of the Lua closure f2.")

(global io {})
(fn io.close [?file]
  "Equivalent to file:close(). Without a file, closes the default output file.")
(fn io.flush []
  "Equivalent to io.output():flush().")
(fn io.input [?file]
  "When called with a file name, it opens the named file (in text mode), and sets its handle as the default input file.")
(fn io.lines [?filename & ...]
  "Opens the given file name in read mode and returns an iterator function that works like file:lines(...).")
(fn io.open [filename ?mode]
  "Opens a file in the given mode. Returns a new file handle, or, in case of errors, nil plus an error message.")
(fn io.output [?file]
  "Similar to io.input, but operates on the default output file.")
(fn io.popen [prog ?mode]
  "Starts the program prog in a separated process and returns a file handle that you can use to read from or write to this program.")
(fn io.read [& ...]
  "Equivalent to io.input():read(...).")
(global io.stderr {})
(global io.stdin {})
(global io.stdout {})
(fn io.tmpfile []
  "In case of success, returns a handle for a temporary file opened in update mode.")
(fn io.type [obj]
  "Checks whether obj is a valid file handle.")
(fn io.write [& ...]
  "Equivalent to io.output():write(...).")

(global math {})
(fn math.abs [x]
  "Returns the maximum value between x and -x.")
(fn math.acos [x]
  "Returns the arc cosine of x (in radians).")
(fn math.asin [x]
  "Returns the arc sine of x (in radians).")
(fn math.atan [y ?x]
  "Returns the arc tangent of y/x (in radians).")
(fn math.ceil [x]
  "Returns the smallest integral value greater than or equal to x.")
(fn math.cos [x]
  "Returns the cosine of x (assumed to be in radians).")
(fn math.deg [x]
  "Converts the angle x from radians to degrees.")
(fn math.exp [x]
  "Returns the value e^x (where e is the base of natural logarithms).")
(fn math.floor [x]
  "Returns the largest integral value smaller than or equal to x.")
(fn math.fmod [x y]
  "Returns the remainder of the division of x by y that rounds the quotient towards zero.")
(global math.huge 0)
(fn math.log [x ?base]
  "Returns the logarithm of x in the given base (defaults to e).")
(fn math.max [x & ...]
  "Returns the argument with the maximum value, according to the Lua operator <.")
(global math.maxinteger 0)
(fn math.min [x & ...]
  "Returns the argument with the minimum value, according to the Lua operator <.")
(global math.mininteger 0)
(fn math.modf [x]
  "Returns the integral part of x and the fractional part of x.")
(global math.pi 3.14)
(fn math.rad [x]
  "Converts the angle x from degrees to radians.")
(fn math.random [?m ?n]
  "When called without arguments, returns a pseudo-random float between 0 and 1. When called with two integers m and n, returns a pseudo-random integer in the range [m, n].")
(fn math.randomseed [?x ?y]
  "Sets the seed for the pseudo-random generator.")
(fn math.sin [x]
  "Returns the sine of x (assumed to be in radians).")
(fn math.sqrt [x]
  "Returns the square root of x.")
(fn math.tan [x]
  "Returns the tangent of x (assumed to be in radians).")
(fn math.tointeger [x]
  "If the value x can be converted to an integer, returns that integer. Otherwise, returns nil.")
(fn math.type [x]
  "Returns \"integer\" if x is an integer, \"float\" if it is a float, or nil if x is not a number.")
(fn math.ult [m n]
  "Returns a boolean, true if and only if integer m is below integer n when they are compared as unsigned integers.")

(global os {})
(fn os.clock []
  "Returns an approximation that the program spent in CPU time in seconds.")
(fn os.date [?format ?time]
  "Returns a string or a table containing date and time, formatted according to the given string format.")
(fn os.difftime [t2 t1]
  "Returns the difference, in seconds, from time t1 to time t2.")
(fn os.execute [?command]
  "Passes command to be executed by an operating system shell.")
(fn os.exit [?code ?close]
  "Calls the ISO C function exit to terminate the host program.")
(fn os.getenv [varname]
  "Returns the value of the process environment variable varname.")
(fn os.remove [filename]
  "Deletes the file (or empty directory) with the given name.")
(fn os.rename [oldname newname]
  "Renames the file or directory named oldname to newname.")
(fn os.setlocale [locale ?category]
  "Sets the current locale of the program.")
(fn os.time [?table]
  "Returns the current time when called without arguments, or a time representing the local date and time specified by the given table.")
(fn os.tmpname []
  "Returns a string with a file name that can be used for a temporary file.")

(global package {})
(global package.config "")
(global package.cpath "")
(global package.loaded {})
(fn package.loadlib [libname funcname]
  "Dynamically links the host program with the C library libname.")
(global package.path "")
(global package.preload {})
(global package.searchers [])
(fn package.searchpath [name path ?sep ?rep]
  "Searches for the given name in the given path.")

(global string {})
(fn string.byte [s ?i ?j]
  "Returns the internal numeric codes of the characters s[i], s[i+1], ..., s[j].")
(fn string.char [& ...]
  "Receives zero or more integers. Returns a string with length equal to the number of arguments, where each character has the internal numeric code equal to its corresponding argument.")
(fn string.dump [f ?strip]
  "Returns a string containing a binary representation (a binary chunk) of the given function.")
(fn string.find [s pattern ?init ?plain]
  "Looks for the first match of pattern in the string s.")
(fn string.format [formatstring & ...]
  "Returns a formatted version of its variable number of arguments following the description given in its first argument.")
(fn string.gmatch [s pattern ?init]
  "Returns an iterator function that, each time it is called, returns the next captures from pattern over the string s.")
(fn string.gsub [s pattern repl ?n]
  "Returns a copy of s in which all (or the first n, if given) occurrences of the pattern have been replaced by a replacement specified by repl.")
(fn string.len [s]
  "Receives a string and returns its length.")
(fn string.lower [s]
  "Receives a string and returns a copy of this string with all uppercase letters changed to lowercase.")
(fn string.match [s pattern ?init]
  "Looks for the first match of pattern in the string s. If it finds one, then match returns the captures from the pattern; otherwise it returns nil.")
(fn string.pack [fmt v1 v2 & ...]
  "Returns a binary string containing the values v1, v2, etc. serialized according to the format string fmt.")
(fn string.packsize [fmt]
  "Returns the size of a string resulting from string.pack with the given format.")
(fn string.rep [s n ?sep]
  "Returns a string that is the concatenation of n copies of the string s separated by the string sep.")
(fn string.reverse [s]
  "Returns a string that is the string s reversed.")
(fn string.sub [s i ?j]
  "Returns the substring of s that starts at i and continues until j.")
(fn string.unpack [fmt s ?pos]
  "Returns the values packed in string s according to the format string fmt.")
(fn string.upper [s]
  "Receives a string and returns a copy of this string with all lowercase letters changed to uppercase.")

(global table {})
(fn table.concat [list ?sep ?i ?j]
  "Given a list where all elements are strings or numbers, returns the string list[i]..sep..list[i+1] ... sep..list[j].")
(fn table.insert [list ?pos value]
  "Inserts element value at position pos in list, shifting up the elements list[pos], list[pos+1], ..., list[#list].")
(fn table.move [a1 f e t ?a2]
  "Moves elements from table a1 to table a2, performing the equivalent to a2[t+i-f] = a1[i] for f <= i <= e.")
(fn table.pack [& ...]
  "Returns a new table with all arguments stored into keys 1, 2, etc. and with a field \"n\" with the total number of arguments.")
(fn table.remove [list ?pos]
  "Removes from list the element at position pos, returning the value of the removed element.")
(fn table.sort [list ?comp]
  "Sorts list elements in a given order, in-place, from list[1] to list[#list].")
(fn table.unpack [list ?i ?j]
  "Returns the elements from the given list. By default, i is 1 and j is #list.")

(global utf8 {})
(fn utf8.char [& ...]
  "Receives zero or more integers, converts each one to its corresponding UTF-8 byte sequence and returns a string with the concatenation of all these sequences.")
(global utf8.charpattern "")
(fn utf8.codes [s ?lax]
  "Returns an iterator function so that the construction `for p, c in utf8.codes(s) do` iterates over all UTF-8 characters in string s.")
(fn utf8.codepoint [s ?i ?j ?lax]
  "Returns the codepoints (as integers) from all characters in s that start between byte position i and j.")
(fn utf8.len [s ?i ?j ?lax]
  "Returns the number of UTF-8 characters in string s that start between positions i and j.")
(fn utf8.offset [s n ?i]
  "Returns the position (in bytes) where the encoding of the n-th character of s (counting from position i) starts.")
