class CalendarCli < Formula
  desc "TUI calendar app with Google Calendar sync"
  homepage "https://github.com/ynui/calander"
  license "MIT"

  head "https://github.com/ynui/calander.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "calendar-cli", shell_output("#{bin}/calendar-cli --help")
  end
end
