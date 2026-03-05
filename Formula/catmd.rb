class Catmd < Formula
  desc "Cat-like CLI that renders Markdown with ANSI styling"
  homepage "https://github.com/schneidermayer/catmd"
  url "https://github.com/schneidermayer/catmd/archive/refs/tags/v0.1.1.tar.gz"
  sha256 "0eaf21bc034886a0079a0a47112c173cf0e4686f5189bae9196eeee1972a467a"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: ".")
  end

  test do
    assert_match "catmd", shell_output("#{bin}/catmd --help")
  end
end
