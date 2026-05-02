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
(fn coroutine.create [f])
(fn coroutine.isyieldable [])
(fn coroutine.resume [co & ...])
(fn coroutine.running [])
(fn coroutine.status [co])
(fn coroutine.wrap [f])
(fn coroutine.yield [& ...])
(fn coroutine.close [co])

(global debug {})
(fn debug.debug [])
(fn debug.gethook [?thread])
(fn debug.getinfo [f ?what])
(fn debug.getlocal [f local])
(fn debug.getmetatable [value])
(fn debug.getregistry [])
(fn debug.getupvalue [f up])
(fn debug.getuservalue [u n])
(fn debug.sethook [hook mask ?count])
(fn debug.setlocal [level local value])
(fn debug.setmetatable [value table])
(fn debug.setupvalue [f up value])
(fn debug.setuservalue [udata value n])
(fn debug.traceback [?message ?level])
(fn debug.upvalueid [f n])
(fn debug.upvaluejoin [f1 n1 f2 n2])

(global io {})
(fn io.close [?file])
(fn io.flush [])
(fn io.input [?file])
(fn io.lines [?filename & ...])
(fn io.open [filename ?mode])
(fn io.output [?file])
(fn io.popen [prog ?mode])
(fn io.read [& ...])
(global io.stderr {})
(global io.stdin {})
(global io.stdout {})
(fn io.tmpfile [])
(fn io.type [obj])
(fn io.write [& ...])

(global math {})
(fn math.abs [x])
(fn math.acos [x])
(fn math.asin [x])
(fn math.atan [y ?x])
(fn math.ceil [x])
(fn math.cos [x])
(fn math.deg [x])
(fn math.exp [x])
(fn math.floor [x])
(fn math.fmod [x y])
(global math.huge 0)
(fn math.log [x ?base])
(fn math.max [x & ...])
(global math.maxinteger 0)
(fn math.min [x & ...])
(global math.mininteger 0)
(fn math.modf [x])
(global math.pi 3.14)
(fn math.rad [x])
(fn math.random [?m ?n])
(fn math.randomseed [?x ?y])
(fn math.sin [x])
(fn math.sqrt [x])
(fn math.tan [x])
(fn math.tointeger [x])
(fn math.type [x])
(fn math.ult [m n])

(global os {})
(fn os.clock [])
(fn os.date [?format ?time])
(fn os.difftime [t2 t1])
(fn os.execute [?command])
(fn os.exit [?code ?close])
(fn os.getenv [varname])
(fn os.remove [filename])
(fn os.rename [oldname newname])
(fn os.setlocale [locale ?category])
(fn os.time [?table])
(fn os.tmpname [])

(global package {})
(global package.config "")
(global package.cpath "")
(global package.loaded {})
(fn package.loadlib [libname funcname])
(global package.path "")
(global package.preload {})
(global package.searchers [])
(fn package.searchpath [name path ?sep ?rep])

(global string {})
(fn string.byte [s ?i ?j])
(fn string.char [& ...])
(fn string.dump [f ?strip])
(fn string.find [s pattern ?init ?plain])
(fn string.format [formatstring & ...])
(fn string.gmatch [s pattern])
(fn string.gsub [s pattern repl ?n])
(fn string.len [s])
(fn string.lower [s])
(fn string.match [s pattern ?init])
(fn string.pack [fmt v1 v2 & ...])
(fn string.packsize [fmt])
(fn string.rep [s n ?sep])
(fn string.reverse [s])
(fn string.sub [s i ?j])
(fn string.unpack [fmt s ?pos])
(fn string.upper [s])

(global table {})
(fn table.concat [list ?sep ?i ?j])
(fn table.insert [list ?pos value])
(fn table.move [a1 f e t ?a2])
(fn table.pack [& ...])
(fn table.remove [list ?pos])
(fn table.sort [list ?comp])
(fn table.unpack [list ?i ?j])

(global utf8 {})
(fn utf8.char [& ...])
(global utf8.charpattern "")
(fn utf8.codes [s])
(fn utf8.codepoint [s ?i ?j])
(fn utf8.len [s ?i ?j])
(fn utf8.offset [s n ?i])
