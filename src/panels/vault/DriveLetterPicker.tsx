interface DriveLetterPickerProps {
  id: string;
  value: string;
  letters: string[];
  onChange: (letter: string) => void;
  onKeyDown?: React.KeyboardEventHandler<HTMLButtonElement>;
}

const FALLBACK_LETTERS = "DEFGHIJKLMNOPQRSTUVWXYZ".split("");

export default function DriveLetterPicker({ id, value, letters, onChange, onKeyDown }: DriveLetterPickerProps) {
  const choices = letters.length ? letters : FALLBACK_LETTERS;

  return (
    <div id={id} className="drive-letter-picker" role="radiogroup" aria-label="Drive letter">
      {choices.map((letter) => (
        <button
          key={letter}
          type="button"
          role="radio"
          aria-checked={value === letter}
          className={`drive-letter-picker__option ${value === letter ? "drive-letter-picker__option--selected" : ""}`}
          onClick={() => onChange(letter)}
          onKeyDown={onKeyDown}
        >
          {letter}:
        </button>
      ))}
    </div>
  );
}
