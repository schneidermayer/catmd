class Catmd < Formula
  desc "Cat-like CLI that renders Markdown with ANSI styling"
  homepage "https://github.com/schneidermayer/catmd"
  url "https://github.com/schneidermayer/catmd/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "20c32fae867fefe3f80df0a1f9e4b5ca20f669e27bcaa8c8ea052a447393caa4"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: ".")
  end

  test do
    assert_match "catmd", shell_output("#{bin}/catmd --help")
  end
end
