(def
  body: (_) @function.inside) @function.around

parameters: ((_) @parameter.inside . ","? @parameter.around) @parameter.around

arguments: ((_) @parameter.inside . ","? @parameter.around) @parameter.around

(comment) @comment.inside

(comment)+ @comment.around
