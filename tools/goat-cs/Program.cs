// goat-cs — C# scripts → ES modules for GoatEngine's JavaScript host.
//
//   goat-cs <assets-root>            translate every .cs under the root (→ X.cs.js next to X.cs)
//   goat-cs <assets-root> a.cs b.cs  only these (paths relative to the root or absolute)
//   goat-cs --api <dir>              write GoatEngine.cs (the API, for IDEs) into <dir>
//
// Scripts are type-checked (Roslyn) against Api/GoatEngine.cs; errors are printed as
// `path(line,col): error CSxxxx: message` and the exit code is 1.
using System.Reflection;
using System.Text;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;

namespace GoatCs;

public static class Program
{
    public static int Main(string[] args)
    {
        if (args.Length == 0)
        {
            Console.Error.WriteLine("usage: goat-cs <assets-root> [files...] | goat-cs --api <dir>");
            return 2;
        }
        if (args[0] == "--api")
        {
            var dir = args.Length > 1 ? args[1] : ".";
            Directory.CreateDirectory(dir);
            File.WriteAllText(Path.Combine(dir, "GoatEngine.cs"), ApiSource());
            return 0;
        }
        var root = Path.GetFullPath(args[0]);
        var files = args.Length > 1
            ? args.Skip(1).Select(f => Path.GetFullPath(Path.IsPathRooted(f) ? f : Path.Combine(root, f))).ToList()
            : Directory.EnumerateFiles(root, "*.cs", SearchOption.AllDirectories).ToList();
        var result = Transpiler.Run(files, root);
        foreach (var d in result.Errors) Console.Error.WriteLine(d);
        foreach (var (file, js) in result.Outputs)
        {
            File.WriteAllText(file + ".js", js);
            Console.WriteLine($"{Path.GetRelativePath(root, file).Replace('\\', '/')} -> {Path.GetFileName(file)}.js");
        }
        return result.Errors.Count > 0 ? 1 : 0;
    }

    public static string ApiSource()
    {
        using var s = Assembly.GetExecutingAssembly().GetManifestResourceStream("GoatEngine.cs")!;
        using var r = new StreamReader(s);
        return r.ReadToEnd();
    }
}

public record TranspileResult(List<(string File, string Js)> Outputs, List<string> Errors);

public static class Transpiler
{
    public static TranspileResult Run(IEnumerable<string> files, string root)
    {
        var parse = new CSharpParseOptions(LanguageVersion.Latest);
        var trees = files.Select(f => CSharpSyntaxTree.ParseText(File.ReadAllText(f), parse, f)).ToList();
        return Run(trees, root);
    }

    public static TranspileResult Run(List<SyntaxTree> trees, string root)
    {
        var parse = new CSharpParseOptions(LanguageVersion.Latest);
        var api = CSharpSyntaxTree.ParseText(Program.ApiSource(), parse, "GoatEngine.cs");
        var refs = ((string)AppContext.GetData("TRUSTED_PLATFORM_ASSEMBLIES")!).Split(Path.PathSeparator)
            .Where(p => Path.GetFileName(p) is var n && (n.StartsWith("System.") || n == "mscorlib.dll" || n == "netstandard.dll" || n.StartsWith("Microsoft.CSharp")))
            .Select(p => MetadataReference.CreateFromFile(p));
        var compilation = CSharpCompilation.Create("Scripts", trees.Append(api), refs,
            new CSharpCompilationOptions(OutputKind.DynamicallyLinkedLibrary, nullableContextOptions: NullableContextOptions.Enable));
        var errors = compilation.GetDiagnostics()
            .Where(d => d.Severity == DiagnosticSeverity.Error && d.Location.SourceTree != api)
            .Select(d =>
            {
                var span = d.Location.GetLineSpan();
                var path = Path.GetRelativePath(root, span.Path).Replace('\\', '/');
                return $"{path}({span.StartLinePosition.Line + 1},{span.StartLinePosition.Character + 1}): error {d.Id}: {d.GetMessage()}";
            })
            .ToList();
        var outputs = new List<(string, string)>();
        if (errors.Count > 0) return new TranspileResult(outputs, errors);
        foreach (var tree in trees)
        {
            var w = new JsWriter(compilation.GetSemanticModel(tree), Path.GetFileNameWithoutExtension(tree.FilePath));
            try
            {
                outputs.Add((tree.FilePath, w.File((CompilationUnitSyntax)tree.GetRoot())));
            }
            catch (UnsupportedException e)
            {
                var span = e.Node.GetLocation().GetLineSpan();
                errors.Add($"{Path.GetRelativePath(root, tree.FilePath).Replace('\\', '/')}({span.StartLinePosition.Line + 1},{span.StartLinePosition.Character + 1}): error GOAT001: {e.Message}");
            }
        }
        return new TranspileResult(outputs, errors);
    }
}

public class UnsupportedException(SyntaxNode node, string what) : Exception($"not supported by goat-cs yet: {what}")
{
    public SyntaxNode Node { get; } = node;
}
